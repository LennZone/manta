//! Left sidebar: favorites, local locations/devices, active connections.

use std::path::PathBuf;

use eframe::egui::{self, CornerRadius, RichText, Sense, Stroke};

use crate::app::Action;
use crate::commands::Cmd;
use crate::favorites::{Favorite, Favorites};
use crate::icons;
use crate::pane::DragPayload;
use crate::theme::{self, pal, radius, size, space, text};
use crate::widgets;
use manta_core::VfsPath;

#[derive(Clone)]
pub struct Place {
    pub icon: &'static str,
    pub name: String,
    pub path: PathBuf,
    /// A mounted network share (SMB, AFP, NFS, WebDAV): ejectable.
    pub network: bool,
}

/// Standard user folders plus `/` and mounted volumes. Reads `/Volumes`, which
/// is a cheap local directory listing; it is only refreshed on demand.
pub fn discover_places() -> (Vec<Place>, Vec<Place>) {
    let mut locations = Vec::new();
    let mut push = |icon, name: &str, p: Option<PathBuf>| {
        if let Some(path) = p.filter(|p| p.is_dir()) {
            locations.push(Place {
                icon,
                name: name.to_string(),
                path,
                network: false,
            });
        }
    };
    push(icons::HOUSE, "Home", dirs::home_dir());
    push(icons::DESKTOP, "Desktop", dirs::desktop_dir());
    push(icons::FILE_TEXT, "Documents", dirs::document_dir());
    push(icons::DOWNLOAD_SIMPLE, "Downloads", dirs::download_dir());

    let mut devices = vec![Place {
        icon: icons::HARD_DRIVES,
        name: "Macintosh HD".into(),
        path: PathBuf::from("/"),
        network: false,
    }];
    let network: Vec<PathBuf> = manta_vfs::volumes::network_volumes()
        .into_iter()
        .map(|v| v.path)
        .collect();
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        let mut vols: Vec<Place> = rd
            .filter_map(Result::ok)
            .filter(|e| {
                // Skip the symlink to the boot volume.
                e.file_type().map(|t| !t.is_symlink()).unwrap_or(false)
            })
            .map(|e| {
                let path = e.path();
                let network = network.contains(&path);
                Place {
                    icon: if network {
                        icons::NETWORK
                    } else {
                        icons::HARD_DRIVE
                    },
                    name: e.file_name().to_string_lossy().into_owned(),
                    path,
                    network,
                }
            })
            .collect();
        vols.sort_by(|a, b| a.name.cmp(&b.name));
        devices.extend(vols);
    }
    (locations, devices)
}

/// Display name of a standard location (discovered with English names so
/// switching the language needs no rescan).
fn place_label(name: &str) -> &str {
    match name {
        "Home" => t!("sidebar.place.home"),
        "Desktop" => t!("sidebar.place.desktop"),
        "Documents" => t!("sidebar.place.documents"),
        "Downloads" => t!("sidebar.place.downloads"),
        _ => name,
    }
}

/// Sidebar glyph for a server connection: a cloud for object storage.
pub fn protocol_icon(p: manta_core::Protocol) -> &'static str {
    match p {
        manta_core::Protocol::S3 => icons::CLOUD,
        manta_core::Protocol::GoogleDrive => icons::GOOGLE_DRIVE_LOGO,
        manta_core::Protocol::Docker => icons::SHIPPING_CONTAINER,
        manta_core::Protocol::Kubernetes => icons::STEERING_WHEEL,
        manta_core::Protocol::Smb => icons::NETWORK,
        _ => icons::GLOBE,
    }
}

/// The Trash row at the end of "Locations".
pub struct TrashRow {
    pub dir: PathBuf,
    /// Items in it, when known (filled icon when > 0).
    pub items: Option<usize>,
}

/// Where rows were drawn (drops and tests).
#[derive(Default)]
pub struct SidebarRects {
    pub favorites: Vec<egui::Rect>,
    pub trash: Option<egui::Rect>,
}

pub struct ConnectionInfo {
    pub id: u64,
    pub label: String,
    /// host, port, user, protocol (to match favorites).
    pub key: (String, u16, String, manta_core::Protocol),
    /// SMB: where the share is mounted (left out of "Devices").
    pub mount: Option<PathBuf>,
}

/// Tooltip of a server favorite: protocol, account, server and what it
/// opens (bucket, share, WebDAV folder).
pub fn favorite_hover(config: &manta_core::ConnectionConfig) -> String {
    use manta_core::Protocol;
    match config.protocol {
        Protocol::S3 => {
            let bucket = match config.s3.as_ref().map(|s| s.bucket.as_str()) {
                Some(b) if !b.is_empty() => b,
                _ => t!("sidebar.all_buckets"),
            };
            format!("S3 {} · {bucket}", config.host)
        }
        Protocol::Docker => t!("sidebar.docker_container", name = config.host),
        Protocol::Kubernetes => t!(
            "sidebar.kubernetes_pod",
            name = config.host,
            namespace = config.username
        ),
        Protocol::Smb => {
            let share = config.smb.as_ref().map_or("", |s| s.share.as_str());
            let who = if config.username.is_empty() {
                String::new()
            } else {
                format!("{}@", config.username)
            };
            format!("SMB {who}{}/{share}", config.host)
        }
        Protocol::WebDav | Protocol::WebDavs => {
            let root = config
                .webdav
                .as_ref()
                .map(|w| manta_vfs::webdav::normalize_root(&w.root))
                .unwrap_or_default();
            let scheme = if config.protocol == Protocol::WebDavs {
                "https"
            } else {
                "http"
            };
            let who = if config.username.is_empty() {
                String::new()
            } else {
                format!("{}@", config.username)
            };
            format!("WebDAV {scheme}://{who}{}{root}/", config.host)
        }
        _ => format!(
            "{} {}@{}:{}",
            config.protocol, config.username, config.host, config.port
        ),
    }
}

/// What the active pane shows, to highlight the matching sidebar row.
pub struct Current {
    /// Local path of the active pane (if local).
    pub local: Option<String>,
    /// Connection id of the active pane (if remote).
    pub conn: Option<u64>,
}

/// Section header with an optional trailing icon button.
pub(crate) fn section(ui: &mut egui::Ui, title: &str, button: Option<(&str, &str, &str)>) -> bool {
    ui.add_space(space::LG);
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.add_space(space::SM);
        widgets::section_title(ui, title);
        if let Some((glyph, tip, shortcut)) = button {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(space::XS);
                let r = widgets::mini_icon_button(ui, glyph, pal(ui).text_tertiary);
                clicked = widgets::tooltip(r, tip, shortcut).clicked();
            });
        }
    });
    ui.add_space(space::XXS);
    clicked
}

#[derive(Default)]
pub(crate) struct Row<'a> {
    pub icon: &'a str,
    pub label: &'a str,
    pub selected: bool,
    /// Green "connected" dot.
    pub status_dot: bool,
    /// Show an eject button on hover.
    pub eject: bool,
    /// Tooltip (and spoken name) of the eject button; default "Disconnect".
    pub eject_tip: Option<&'a str>,
    /// Dot colour instead of green (with `status_dot`).
    pub dot: Option<egui::Color32>,
    /// Dim text after the label (e.g. a container's image).
    pub detail: Option<&'a str>,
    /// Greyed out (stopped containers).
    pub dimmed: bool,
    /// Extra left indent (nested rows).
    pub indent: f32,
    /// The row can be dragged (favorites re-sort).
    pub draggable: bool,
    /// Briefly highlighted (0 = off, 1 = full): a favorite that was
    /// dropped again.
    pub flash: f32,
    /// Draw the icon filled (the Trash when it has items, like Finder).
    pub fill_icon: bool,
    /// A tag: a coloured circle instead of the icon (hollow without colour).
    pub tag_dot: Option<egui::Color32>,
}

/// The sidebar's Tags section: the user's tags and the one being shown.
#[derive(Default)]
pub struct TagRows {
    pub tags: Vec<crate::tags::Tag>,
    /// The tag whose search the active pane shows.
    pub current: Option<String>,
}

/// Returns (row response, eject clicked).
pub(crate) fn row(ui: &mut egui::Ui, r: Row<'_>) -> (egui::Response, bool) {
    let p = pal(ui);
    let w = ui.available_width();
    let sense = if r.draggable {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, size::SIDEBAR_ROW), sense);
    // VoiceOver: "Projects, button, selected" / "web-01, connected". The
    // tooltip (path, server) becomes the description.
    let mut spoken = r.label.to_string();
    if let Some(d) = r.detail.filter(|d| !d.is_empty()) {
        spoken.push_str(", ");
        spoken.push_str(d);
    }
    if r.status_dot && r.dot.is_none() {
        spoken.push_str(", ");
        spoken.push_str(t!("a11y.connected"));
    }
    crate::a11y::name(&resp, egui::accesskit::Role::Button, &spoken);
    if r.selected {
        resp.ctx
            .accesskit_node_builder(resp.id, |n| n.set_selected(true));
    }
    let hovered = resp.hovered();
    if r.flash > 0.0 {
        let a = r.flash.clamp(0.0, 1.0);
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(radius::ROW + 1),
            p.accent.gamma_multiply(0.28 * a),
        );
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(radius::ROW + 1),
            Stroke::new(1.5, p.accent.gamma_multiply(a)),
            egui::StrokeKind::Inside,
        );
    } else if r.selected {
        let fill = if p.dark {
            egui::Color32::from_rgb(64, 64, 70)
        } else {
            egui::Color32::from_rgb(212, 212, 218)
        };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius::ROW + 1), fill);
    } else if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius::ROW + 1), p.hover);
    }
    let ir = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 16.0 + r.indent, rect.center().y),
        egui::vec2(18.0, 18.0),
    );
    let icon_color = if r.dimmed {
        p.text_tertiary
    } else {
        p.accent_text
    };
    match r.tag_dot {
        Some(c) if c == egui::Color32::TRANSPARENT => {
            ui.painter()
                .circle_stroke(ir.center(), 5.5, Stroke::new(1.2, p.text_secondary));
        }
        Some(c) => {
            ui.painter().circle_filled(ir.center(), 6.0, c);
        }
        None => widgets::paint_icon(ui, ir, r.icon, 16.0, icon_color, r.fill_icon),
    }
    if r.status_dot {
        let c = ir.right_bottom() - egui::vec2(1.0, 2.0);
        ui.painter().circle_filled(
            c,
            4.0,
            if r.selected {
                p.selection_inactive
            } else {
                p.sidebar
            },
        );
        ui.painter()
            .circle_filled(c, 2.8, r.dot.unwrap_or(p.success));
    }
    let right_pad = if r.eject { 26.0 } else { space::XXS };
    let font = if r.selected {
        theme::medium(text::BODY)
    } else {
        theme::regular(text::BODY)
    };
    let text_color = if r.dimmed { p.text_secondary } else { p.text };
    let mut job = egui::text::LayoutJob::default();
    job.append(r.label, 0.0, egui::TextFormat::simple(font, text_color));
    if let Some(d) = r.detail.filter(|d| !d.is_empty()) {
        job.append(
            d,
            6.0,
            egui::TextFormat::simple(theme::regular(text::CAPTION), p.text_tertiary),
        );
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(
        (rect.right() - right_pad - (ir.right() + 8.0)).max(10.0),
    );
    let g = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(ir.right() + 7.0, rect.center().y - g.size().y / 2.0),
        g,
        p.text,
    );
    let mut eject = false;
    if r.eject {
        let er = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 13.0, rect.center().y),
            egui::vec2(20.0, 20.0),
        );
        let e = ui.interact(er, resp.id.with("eject"), Sense::click());
        let show = hovered || e.hovered() || r.selected;
        if e.hovered() {
            ui.painter()
                .rect_filled(er, CornerRadius::same(radius::CONTROL), p.hover);
        }
        if show {
            widgets::paint_icon(
                ui,
                er,
                icons::EJECT,
                13.0,
                if e.hovered() {
                    p.text
                } else {
                    p.text_secondary
                },
                false,
            );
        }
        let tip = r.eject_tip.unwrap_or(t!("sidebar.disconnect"));
        eject = widgets::tooltip(e, tip, "").clicked();
    }
    (resp, eject)
}

#[allow(clippy::too_many_arguments)]
pub fn sidebar_ui(
    ui: &mut egui::Ui,
    favorites: &Favorites,
    locations: &[Place],
    devices: &[Place],
    trash: Option<&TrashRow>,
    cloud: &[manta_vfs::cloud_local::CloudLocation],
    drive_hint: bool,
    connections: &[ConnectionInfo],
    current: &Current,
    containers: &mut crate::containers::Containers,
    flash: Option<(usize, f32)>,
    tags: &TagRows,
    actions: &mut Vec<Action>,
) -> SidebarRects {
    containers.poll(ui.ctx());
    let mut trash_rect = None;
    let mut fav_rows: Vec<egui::Rect> = Vec::with_capacity(favorites.items.len());
    let active_key = current
        .conn
        .and_then(|id| connections.iter().find(|c| c.id == id))
        .map(|c| c.key.clone());
    let p = pal(ui);
    let is_current = |path: &PathBuf| current.local.as_deref() == Some(&*path.to_string_lossy());
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;

            let fav_top = ui.cursor().top();
            let fav_x = ui.available_rect_before_wrap().x_range();
            if section(
                ui,
                t!("sidebar.favorites"),
                Some((
                    icons::PLUS,
                    t!("sidebar.add_current"),
                    Cmd::AddToFavorites.shortcut(),
                )),
            ) {
                actions.push(Action::AddCurrentToFavorites);
            }
            if favorites.items.is_empty() {
                // An indented block (not `horizontal_wrapped`, whose wrapped
                // lines start at the left edge, left of the first line).
                ui.horizontal(|ui| {
                    ui.add_space(space::SM);
                    ui.add(
                        egui::Label::new(
                            RichText::new(t!(
                                "sidebar.favorites_empty",
                                shortcut = Cmd::AddToFavorites.shortcut()
                            ))
                            .font(theme::regular(text::CAPTION))
                            .color(p.text_secondary),
                        )
                        .wrap(),
                    );
                });
            }
            for (i, fav) in favorites.items.iter().enumerate() {
                let (icon, hover, selected, connected) = match fav {
                    Favorite::Connection { config, .. } => {
                        let connected = connections.iter().any(|c| {
                            c.key
                                == (
                                    config.host.clone(),
                                    config.port,
                                    config.username.clone(),
                                    config.protocol,
                                )
                        });
                        let hover = favorite_hover(config);
                        (protocol_icon(config.protocol), hover, false, connected)
                    }
                    Favorite::Local { path, .. } => (
                        icons::FOLDER_SIMPLE_STAR,
                        path.clone(),
                        current.local.as_deref() == Some(path.as_str()),
                        false,
                    ),
                };
                let name = fav.display_name();
                let (resp, _) = row(
                    ui,
                    Row {
                        icon,
                        label: &name,
                        selected,
                        status_dot: connected,
                        eject: false,
                        draggable: true,
                        flash: flash.filter(|(f, _)| *f == i).map_or(0.0, |(_, a)| a),
                        ..Row::default()
                    },
                );
                fav_rows.push(resp.rect);
                if resp.drag_started() {
                    egui::DragAndDrop::set_payload(ui.ctx(), FavoriteDrag(i));
                }
                // No tooltip while something is dragged over the list.
                let dragging = egui::DragAndDrop::has_any_payload(ui.ctx());
                let resp = if dragging {
                    resp
                } else {
                    resp.on_hover_text(hover)
                };
                if resp.clicked() {
                    actions.push(Action::OpenFavorite(i));
                }
                resp.context_menu(|ui| {
                    if ui
                        .button(format!("{}   {}", icons::FOLDER_OPEN, t!("sidebar.open")))
                        .clicked()
                    {
                        actions.push(Action::OpenFavorite(i));
                        ui.close();
                    }
                    if matches!(fav, Favorite::Connection { .. })
                        && ui
                            .button(format!(
                                "{}   {}",
                                icons::PENCIL_SIMPLE,
                                t!("sidebar.edit_connection")
                            ))
                            .clicked()
                    {
                        actions.push(Action::EditFavorite(i));
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(format!("{}   {}", icons::X, t!("sidebar.remove_favorite")))
                        .clicked()
                    {
                        actions.push(Action::RemoveFavorite(i));
                        ui.close();
                    }
                });
            }

            let fav_bottom = ui.cursor().top() + space::XS;
            let fav_area = egui::Rect::from_x_y_ranges(
                fav_x,
                egui::Rangef::new(fav_top + space::SM, fav_bottom),
            );
            favorites_drop(ui, fav_area, &fav_rows, actions);

            section(ui, t!("sidebar.locations"), None);
            for pl in locations {
                let (resp, _) = row(
                    ui,
                    Row {
                        icon: pl.icon,
                        label: place_label(&pl.name),
                        selected: is_current(&pl.path),
                        status_dot: false,
                        eject: false,
                        ..Row::default()
                    },
                );
                if resp.on_hover_text(pl.path.display().to_string()).clicked() {
                    actions.push(Action::OpenLocal(pl.path.clone()));
                }
            }
            if let Some(t) = trash {
                trash_rect = Some(trash_row(ui, t, is_current(&t.dir), actions));
            }

            crate::cloud::section_ui(
                ui,
                cloud,
                drive_hint,
                &|p| current.local.as_deref() == Some(&*p.to_string_lossy()),
                actions,
            );

            if section(
                ui,
                t!("sidebar.devices"),
                Some((icons::ARROW_CLOCKWISE, t!("sidebar.rescan"), "")),
            ) {
                actions.push(Action::RescanVolumes);
            }
            for pl in devices {
                // An SMB connection's share is listed under "Servers".
                if connections
                    .iter()
                    .any(|c| c.mount.as_deref() == Some(pl.path.as_path()))
                {
                    continue;
                }
                let (resp, eject) = row(
                    ui,
                    Row {
                        icon: pl.icon,
                        label: place_label(&pl.name),
                        selected: is_current(&pl.path),
                        status_dot: false,
                        eject: pl.network,
                        eject_tip: Some(t!("sidebar.eject")),
                        ..Row::default()
                    },
                );
                if eject {
                    actions.push(Action::EjectVolume(pl.path.clone()));
                } else if resp.on_hover_text(pl.path.display().to_string()).clicked() {
                    actions.push(Action::OpenLocal(pl.path.clone()));
                }
            }

            if section(
                ui,
                t!("sidebar.servers"),
                Some((icons::PLUS, Cmd::Connect.label(), Cmd::Connect.shortcut())),
            ) {
                actions.push(Action::ShowConnect);
            }
            if connections.is_empty() {
                ui.add_space(space::XXS);
                let (resp, _) = row(
                    ui,
                    Row {
                        icon: icons::PLUGS,
                        label: Cmd::Connect.label(),
                        selected: false,
                        status_dot: false,
                        eject: false,
                        ..Row::default()
                    },
                );
                if widgets::tooltip(resp, t!("sidebar.connect_tip"), Cmd::Connect.shortcut())
                    .clicked()
                {
                    actions.push(Action::ShowConnect);
                }
            }
            for c in connections {
                let (resp, eject) = row(
                    ui,
                    Row {
                        icon: protocol_icon(c.key.3),
                        label: &c.label,
                        selected: current.conn == Some(c.id),
                        status_dot: true,
                        eject: true,
                        ..Row::default()
                    },
                );
                if eject {
                    actions.push(Action::Disconnect(c.id));
                } else if resp
                    .on_hover_text(t!("sidebar.connected", name = c.label))
                    .clicked()
                {
                    actions.push(Action::OpenConnection(c.id));
                }
            }

            if containers.snapshot().any() {
                if section(
                    ui,
                    t!("sidebar.containers"),
                    Some((icons::ARROW_CLOCKWISE, t!("sidebar.refresh_containers"), "")),
                ) {
                    containers.refresh_now();
                }
                crate::containers::section_ui(ui, containers, active_key.as_ref(), actions);
            }

            // Tags (like Finder): a click shows everything with the tag.
            if !tags.tags.is_empty() {
                section(ui, t!("sidebar.tags"), None);
                for t in &tags.tags {
                    let (resp, _) = row(
                        ui,
                        Row {
                            icon: icons::TAG,
                            label: &t.name,
                            selected: tags
                                .current
                                .as_deref()
                                .is_some_and(|c| crate::tags::same_name(c, &t.name)),
                            tag_dot: Some(t.color.rgb(p.dark)),
                            ..Row::default()
                        },
                    );
                    if resp
                        .on_hover_text(t!("sidebar.tag_tip", name = t.name))
                        .clicked()
                    {
                        actions.push(Action::ShowTag(t.name.clone()));
                    }
                }
            }
            ui.add_space(space::LG);
        });
    SidebarRects {
        favorites: fav_rows,
        trash: trash_rect,
    }
}

/// The Trash row (last in "Locations", as in Finder): opens the Trash,
/// "Empty Trash…" in its menu, and a drop target for items (see
/// [`trash_drop`]).
fn trash_row(
    ui: &mut egui::Ui,
    t: &TrashRow,
    selected: bool,
    actions: &mut Vec<Action>,
) -> egui::Rect {
    let (resp, _) = row(
        ui,
        Row {
            icon: icons::TRASH,
            label: t!("sidebar.trash"),
            selected,
            fill_icon: t.items.is_some_and(|n| n > 0),
            ..Row::default()
        },
    );
    let rect = resp.rect;
    let dragging = egui::DragAndDrop::has_any_payload(ui.ctx());
    let tip = match t.items {
        Some(0) => t!("sidebar.trash.empty").to_string(),
        Some(n) => t!("pane.items", count = n),
        None => t.dir.display().to_string(),
    };
    let resp = if dragging {
        resp
    } else {
        resp.on_hover_text(tip)
    };
    if resp.clicked() {
        actions.push(Action::OpenLocal(t.dir.clone()));
    }
    resp.context_menu(|ui| {
        if ui
            .button(format!("{}   {}", icons::FOLDER_OPEN, t!("sidebar.open")))
            .clicked()
        {
            actions.push(Action::OpenLocal(t.dir.clone()));
            ui.close();
        }
        if ui
            .button(format!(
                "{}   {}",
                icons::ARROW_SQUARE_OUT,
                t!("sidebar.trash.show_in_finder")
            ))
            .clicked()
        {
            actions.push(Action::ShowTrashInFinder);
            ui.close();
        }
        ui.separator();
        let empty = egui::Button::new(format!("{}   {}", icons::TRASH, Cmd::EmptyTrash.label()))
            .shortcut_text(Cmd::EmptyTrash.shortcut());
        if ui.add_enabled(t.items != Some(0), empty).clicked() {
            actions.push(Action::EmptyTrash);
            ui.close();
        }
    });
    trash_drop(ui, rect, &t.dir, actions);
    rect
}

/// What a drag would do over the Trash row this frame (read by the pane's
/// drag badge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashHover {
    /// `n` local items go to the Trash.
    Trash(usize),
    /// `n` items on a server: deleted permanently (asks first).
    Delete(usize),
    /// Items in an archive (read-only): refused.
    ReadOnly,
    /// The items are in the Trash already: nothing to do.
    AlreadyThere,
}

impl TrashHover {
    pub fn refused(self) -> bool {
        matches!(self, TrashHover::ReadOnly | TrashHover::AlreadyThere)
    }

    /// The drag badge's text.
    pub fn badge(self) -> String {
        match self {
            TrashHover::Trash(n) => t!("pane.drag.trash", count = n),
            TrashHover::Delete(n) => t!("pane.drag.trash_delete", count = n),
            TrashHover::ReadOnly => t!("pane.drag.trash_read_only").to_string(),
            TrashHover::AlreadyThere => t!("pane.drag.trash_already").to_string(),
        }
    }
}

fn trash_hover_id() -> egui::Id {
    egui::Id::unique("manta-trash-drop-hover")
}

/// What a drag from a pane would do over the Trash row this frame.
pub fn trash_drop_hover(ctx: &egui::Context) -> Option<TrashHover> {
    ctx.data(|d| d.get_temp::<Option<TrashHover>>(trash_hover_id()))
        .flatten()
}

/// The accent ring of a row that takes a drop.
fn drop_ring(ui: &egui::Ui, rect: egui::Rect) {
    let p = pal(ui);
    let rr = CornerRadius::same(radius::ROW + 1);
    ui.painter().rect_filled(
        rect,
        rr,
        p.accent.gamma_multiply(if p.dark { 0.22 } else { 0.14 }),
    );
    ui.painter().rect_stroke(
        rect,
        rr,
        Stroke::new(1.5, p.accent),
        egui::StrokeKind::Inside,
    );
}

/// A pill next to the pointer (drags the panes don't label: favorites and
/// files from other apps).
fn drop_badge(ctx: &egui::Context, pos: egui::Pos2, text: String, refused: bool) {
    let p = theme::pal_ctx(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("trash-drop-badge"),
    ));
    let (bg, fg) = if refused {
        (p.elevated, p.text_secondary)
    } else {
        (p.accent, p.text_on_accent)
    };
    let galley = painter.layout_no_wrap(text, theme::medium(text::SMALL), fg);
    let r = egui::Rect::from_min_size(pos + egui::vec2(14.0, 10.0), galley.size())
        .expand2(egui::vec2(8.0, 4.0));
    painter.rect_filled(r, CornerRadius::same(radius::PILL), bg);
    if refused {
        painter.rect_stroke(
            r,
            CornerRadius::same(radius::PILL),
            Stroke::new(1.0, p.separator),
            egui::StrokeKind::Inside,
        );
    }
    painter.galley(r.min + egui::vec2(8.0, 4.0), galley, fg);
}

/// The Trash row as a drop target: items from a pane (local ones go to the
/// Trash, server items are deleted after asking, archives are refused),
/// files from other apps (to the Trash) and favorites (removed, like
/// dragging them out of Finder's sidebar).
fn trash_drop(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    dir: &std::path::Path,
    actions: &mut Vec<Action>,
) {
    let ctx = ui.ctx().clone();
    ctx.data_mut(|d| d.insert_temp::<Option<TrashHover>>(trash_hover_id(), None));
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let over = pointer.filter(|pos| rect.contains(*pos));
    let released = ctx.input(|i| i.pointer.any_released());

    if let Some(drag) = egui::DragAndDrop::payload::<FavoriteDrag>(&ctx) {
        if let Some(pos) = over {
            drop_ring(ui, rect);
            drop_badge(
                &ctx,
                pos,
                t!("sidebar.trash.remove_favorite").to_string(),
                false,
            );
            if released {
                egui::DragAndDrop::clear_payload(&ctx);
                actions.push(Action::RemoveFavorite(drag.0));
            }
        }
        return;
    }

    if let Some(payload) = egui::DragAndDrop::payload::<DragPayload>(&ctx) {
        let Some(_) = over else { return };
        let n = payload.paths.len();
        let local = |p: &VfsPath| payload.fs.local_path(p);
        let hover = if payload.archive || payload.fs.is_read_only() {
            TrashHover::ReadOnly
        } else if crate::app::can_trash(&payload.fs) {
            if payload
                .paths
                .iter()
                .all(|p| local(p).is_some_and(|l| l.starts_with(dir)))
            {
                TrashHover::AlreadyThere
            } else {
                TrashHover::Trash(n)
            }
        } else {
            TrashHover::Delete(n)
        };
        ctx.data_mut(|d| d.insert_temp::<Option<TrashHover>>(trash_hover_id(), Some(hover)));
        if hover.refused() {
            ctx.set_cursor_icon(egui::CursorIcon::NotAllowed);
            return;
        }
        drop_ring(ui, rect);
        if released {
            if let Some(payload) = egui::DragAndDrop::take_payload::<DragPayload>(&ctx) {
                actions.push(Action::DropOnTrash {
                    from: payload.from,
                    fs: payload.fs.clone(),
                    paths: payload.paths.clone(),
                });
            }
        }
        return;
    }

    // Files from another app (Finder, Mail, …).
    if let Some(h) = crate::native_drag::external_hover(&ctx) {
        if let Some(pos) = h.pos.filter(|p| rect.contains(*p)) {
            drop_ring(ui, rect);
            drop_badge(
                &ctx,
                pos,
                t!("pane.drag.trash", count = h.paths.len()),
                false,
            );
        }
    }
    if let Some(d) = crate::native_drag::take_external_drop_in(&ctx, rect) {
        actions.push(Action::TrashExternal(d.paths));
    }
}

/// Payload while a favorite is dragged to a new position.
pub struct FavoriteDrag(pub usize);

/// What a drag from a pane would do over the favorites this frame (read by
/// the pane's drag badge): `Some(n)` = add `n` folders, `Some(0)` = nothing
/// there is a folder (refused).
pub fn favorites_drop_hover(ctx: &egui::Context) -> Option<usize> {
    ctx.data(|d| d.get_temp::<Option<usize>>(favorites_hover_id()))
        .flatten()
}

fn favorites_hover_id() -> egui::Id {
    egui::Id::unique("manta-favorites-drop-hover")
}

/// Where a drop at height `y` lands among the favorite `rows`: the index
/// of the first row whose middle is below `y` (`rows.len()` = the end).
pub fn insertion_index(rows: &[egui::Rect], y: f32) -> usize {
    rows.iter()
        .position(|r| y < r.center().y)
        .unwrap_or(rows.len())
}

/// The favorites as a drop target: folders dragged from a pane are added
/// (files refused), favorites dragged within the list are re-sorted.
fn favorites_drop(
    ui: &mut egui::Ui,
    area: egui::Rect,
    rows: &[egui::Rect],
    actions: &mut Vec<Action>,
) {
    let ctx = ui.ctx().clone();
    ctx.data_mut(|d| d.insert_temp::<Option<usize>>(favorites_hover_id(), None));
    let p = pal(ui);
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let over = pointer.filter(|pos| area.expand2(egui::vec2(0.0, 4.0)).contains(*pos));
    let released = ctx.input(|i| i.pointer.any_released());
    let painter = ui.painter().clone();
    // The insertion line between two rows (Finder style: a ring and a bar).
    let line = |at: usize| {
        let y = match (at.checked_sub(1).and_then(|i| rows.get(i)), rows.get(at)) {
            (Some(a), Some(b)) => (a.bottom() + b.top()) / 2.0,
            (Some(a), None) => a.bottom() + 1.0,
            (None, Some(b)) => b.top() - 1.0,
            (None, None) => return,
        };
        let left = area.left() + space::SM;
        painter.circle_stroke(egui::pos2(left + 3.0, y), 3.0, Stroke::new(1.5, p.accent));
        painter.line_segment(
            [
                egui::pos2(left + 6.0, y),
                egui::pos2(area.right() - space::XS, y),
            ],
            Stroke::new(2.0, p.accent),
        );
    };

    if let Some(drag) = egui::DragAndDrop::payload::<FavoriteDrag>(&ctx) {
        let from = drag.0;
        if let Some(r) = rows.get(from) {
            // The dragged row fades while it is "in the air".
            painter.rect_filled(
                *r,
                CornerRadius::same(radius::ROW + 1),
                p.sidebar.gamma_multiply(0.55),
            );
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(pos) = over {
            let at = insertion_index(rows, pos.y);
            if at != from && at != from + 1 {
                line(at);
                if released {
                    egui::DragAndDrop::clear_payload(&ctx);
                    actions.push(Action::MoveFavorite(from, at));
                }
            }
        }
        return;
    }

    let Some(payload) = egui::DragAndDrop::payload::<DragPayload>(&ctx) else {
        return;
    };
    let Some(pos) = over else { return };
    let folders = payload.folders();
    ctx.data_mut(|d| d.insert_temp::<Option<usize>>(favorites_hover_id(), Some(folders.len())));
    if folders.is_empty() {
        ctx.set_cursor_icon(egui::CursorIcon::NotAllowed);
        return;
    }
    let rr = CornerRadius::same(radius::ROW + 2);
    painter.rect_filled(
        area,
        rr,
        p.accent.gamma_multiply(if p.dark { 0.14 } else { 0.08 }),
    );
    painter.rect_stroke(
        area,
        rr,
        Stroke::new(1.5, p.accent),
        egui::StrokeKind::Inside,
    );
    let at = insertion_index(rows, pos.y);
    line(at);
    if released {
        if let Some(payload) = egui::DragAndDrop::take_payload::<DragPayload>(&ctx) {
            actions.push(Action::AddFavoriteFolders {
                at,
                conn: payload.conn,
                folders: payload.folders(),
            });
        }
    }
}
