//! Where a pane is, for the location chip at the start of its path bar:
//! „Lokal“ on the Mac, „Docker · shop-web“, „SFTP · deploy@web-01“,
//! „S3 · bucket“, a cloud folder's provider („Google Drive“) or
//! „Archiv · backup.zip“. Anything but „Lokal“ gets a "back to local"
//! button next to the chip.

use manta_core::{ConnectionConfig, Protocol};
use manta_vfs::cloud_local::CloudLocation;

use crate::icons;
use crate::pane::Location;

/// What the location chip shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceChip {
    pub icon: &'static str,
    /// Short text: „Lokal“, „Docker · shop-web“, „Google Drive“.
    pub label: String,
    /// The full text (cloud folders: with the account and the folder).
    pub tooltip: String,
    /// On the Mac, outside cloud folders and archives: no back button.
    pub local: bool,
    /// A cloud folder's top (a click on the chip goes there, not to /).
    pub cloud_root: Option<std::path::PathBuf>,
}

impl PlaceChip {
    fn new(icon: &'static str, label: String, tooltip: String) -> PlaceChip {
        PlaceChip {
            icon,
            label,
            tooltip,
            local: false,
            cloud_root: None,
        }
    }
}

/// "Kind · name", or just the kind when the name is empty.
fn kind_name(kind: &str, name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        kind.to_string()
    } else {
        format!("{kind} · {name}")
    }
}

/// The chip of `loc`. `conn`: its connection (with the backend's label);
/// `cloud`: the synced cloud folder a local location is in.
pub fn place_chip(
    loc: &Location,
    conn: Option<(&ConnectionConfig, &str)>,
    cloud: Option<&CloudLocation>,
) -> PlaceChip {
    if let Some(m) = &loc.archive {
        let label = kind_name(t!("place.archive"), &m.file_name);
        return PlaceChip::new(icons::ARCHIVE, label.clone(), label);
    }
    if loc.conn.is_some() {
        let Some((cfg, backend_label)) = conn else {
            let label = t!("app.disconnected").to_string();
            return PlaceChip::new(icons::GLOBE, label.clone(), label);
        };
        return remote_chip(loc, cfg, backend_label);
    }
    if let Some(c) = cloud {
        let (base, account) = crate::cloud::split_display_name(&c.display_name);
        let mut tooltip = base.to_string();
        if let Some(a) = account {
            tooltip.push_str(&format!(" · {a}"));
        }
        tooltip.push('\n');
        tooltip.push_str(&c.path.display().to_string());
        let mut chip = PlaceChip::new(
            crate::cloud::provider_icon(c.provider),
            base.to_string(),
            tooltip,
        );
        chip.cloud_root = Some(c.path.clone());
        return chip;
    }
    let label = t!("pane.local").to_string();
    PlaceChip {
        icon: icons::HARD_DRIVE,
        tooltip: label.clone(),
        label,
        local: true,
        cloud_root: None,
    }
}

fn remote_chip(loc: &Location, cfg: &ConnectionConfig, backend_label: &str) -> PlaceChip {
    let proto = cfg.protocol;
    let icon = crate::sidebar::protocol_icon(proto);
    let (label, tooltip) = match proto {
        Protocol::Docker => {
            let name = cfg
                .container
                .as_ref()
                .map(|c| c.container.as_str())
                .filter(|n| !n.is_empty())
                .unwrap_or(&cfg.host);
            let l = kind_name("Docker", name);
            (l.clone(), l)
        }
        Protocol::Kubernetes => {
            let (pod, ns) = cfg
                .container
                .as_ref()
                .map(|c| (c.pod.as_str(), c.namespace.as_str()))
                .filter(|(p, _)| !p.is_empty())
                .unwrap_or((cfg.host.as_str(), ""));
            let l = kind_name("Kubernetes", pod);
            let tip = if ns.is_empty() {
                l.clone()
            } else {
                format!("{l} · {ns}")
            };
            (l, tip)
        }
        Protocol::S3 => {
            // A connection for one bucket, else the bucket the pane is in.
            let fixed = cfg
                .s3
                .as_ref()
                .map(|s| s.bucket.clone())
                .filter(|b| !b.is_empty());
            let bucket = fixed.or_else(|| loc.path.components().first().map(|c| c.to_string()));
            let l = kind_name("S3", bucket.as_deref().unwrap_or(&cfg.host));
            (l.clone(), format!("{l}\n{}", cfg.host))
        }
        Protocol::GoogleDrive => {
            let l = "Google Drive".to_string();
            let tip = if cfg.username.is_empty() {
                l.clone()
            } else {
                format!("{l} · {}", cfg.username)
            };
            (l, tip)
        }
        Protocol::Smb => {
            // „SMB · nas/Team“: server and share.
            let share = cfg
                .smb
                .as_ref()
                .map(|s| s.share.trim().trim_matches('/'))
                .unwrap_or("");
            let target = if share.is_empty() {
                cfg.host.clone()
            } else {
                format!("{}/{share}", cfg.host)
            };
            let l = kind_name("SMB", &target);
            let tip = if cfg.username.is_empty() {
                l.clone()
            } else {
                format!("{l} · {}", cfg.username)
            };
            (l, tip)
        }
        Protocol::WebDav | Protocol::WebDavs => {
            let l = kind_name("WebDAV", &cfg.host);
            let mut tip = l.clone();
            if !cfg.username.is_empty() {
                tip.push_str(&format!(" · {}", cfg.username));
            }
            let root = cfg
                .webdav
                .as_ref()
                .map(|w| manta_vfs::webdav::normalize_root(&w.root))
                .unwrap_or_default();
            let scheme = if proto == Protocol::WebDavs {
                "https"
            } else {
                "http"
            };
            tip.push_str(&format!("\n{scheme}://{}{root}/", cfg.host));
            (l, tip)
        }
        Protocol::Sftp | Protocol::Ftp | Protocol::Ftps | Protocol::Local => {
            let who = if cfg.host.is_empty() {
                backend_label.to_string()
            } else if cfg.username.is_empty() || proto != Protocol::Sftp {
                cfg.host.clone()
            } else {
                format!("{}@{}", cfg.username, cfg.host)
            };
            let l = kind_name(&proto.to_string(), &who);
            let tip = if cfg.username.is_empty() || proto == Protocol::Sftp {
                l.clone()
            } else {
                format!("{l} · {}", cfg.username)
            };
            (l, tip)
        }
    };
    PlaceChip::new(icon, label, tooltip)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use manta_core::{Auth, ContainerOptions, S3Options, SharedVfs, VfsPath};
    use manta_vfs::cloud_local;

    use super::*;

    fn cfg(protocol: Protocol, host: &str, user: &str) -> ConnectionConfig {
        ConnectionConfig {
            name: String::new(),
            protocol,
            host: host.into(),
            port: 22,
            username: user.into(),
            auth: Auth::Agent,
            initial_path: None,
            s3: None,
            container: None,
            ftp: None,
            webdav: None,
            smb: None,
        }
    }

    fn remote(path: &str) -> Location {
        let fs: SharedVfs = Arc::new(manta_vfs::local::LocalFs);
        Location::new(fs, Some(1), VfsPath::new(path))
    }

    #[test]
    fn chip_text_per_kind() {
        crate::i18n::with_locale(crate::i18n::Locale::De, || {
            let fs: SharedVfs = Arc::new(manta_vfs::local::LocalFs);
            let local = Location::new(fs, None, VfsPath::new("/Users/me"));
            let c = place_chip(&local, None, None);
            assert_eq!(
                (c.label.as_str(), c.local, c.icon),
                ("Lokal", true, icons::HARD_DRIVE)
            );

            let sftp = cfg(Protocol::Sftp, "web-01", "deploy");
            assert_eq!(
                place_chip(&remote("/var/www"), Some((&sftp, "x")), None).label,
                "SFTP · deploy@web-01"
            );
            let ftp = cfg(Protocol::Ftp, "ftp.example.com", "me");
            let c = place_chip(&remote("/"), Some((&ftp, "x")), None);
            assert_eq!(c.label, "FTP · ftp.example.com");
            assert!(!c.local);

            let mut docker = cfg(Protocol::Docker, "shop-web", "");
            docker.container = Some(Box::new(ContainerOptions {
                container: "shop-web".into(),
                ..Default::default()
            }));
            let c = place_chip(&remote("/app"), Some((&docker, "x")), None);
            assert_eq!(
                (c.label.as_str(), c.icon),
                ("Docker · shop-web", icons::SHIPPING_CONTAINER)
            );

            let mut k8s = cfg(Protocol::Kubernetes, "prod/api-7f9", "");
            k8s.container = Some(Box::new(ContainerOptions {
                pod: "api-7f9".into(),
                namespace: "prod".into(),
                ..Default::default()
            }));
            let c = place_chip(&remote("/"), Some((&k8s, "x")), None);
            assert_eq!(c.label, "Kubernetes · api-7f9");
            assert!(c.tooltip.contains("prod"));

            let s3 = cfg(Protocol::S3, "s3.amazonaws.com", "AKIA");
            assert_eq!(
                place_chip(&remote("/media-bucket/img"), Some((&s3, "x")), None).label,
                "S3 · media-bucket"
            );
            let mut s3b = s3.clone();
            s3b.s3 = Some(Box::new(S3Options {
                bucket: "fixed".into(),
                ..Default::default()
            }));
            assert_eq!(
                place_chip(&remote("/img"), Some((&s3b, "x")), None).label,
                "S3 · fixed"
            );

            let mut smb = cfg(Protocol::Smb, "nas.local", "anna");
            smb.smb = Some(Box::new(manta_core::SmbOptions {
                share: "Team".into(),
            }));
            let c = place_chip(&remote("/Docs"), Some((&smb, "x")), None);
            assert_eq!(
                (c.label.as_str(), c.icon, c.local),
                ("SMB · nas.local/Team", icons::NETWORK, false)
            );
            assert!(c.tooltip.contains("anna"));

            let mut dav = cfg(Protocol::WebDavs, "cloud.example.com", "anna");
            dav.webdav = Some(Box::new(manta_core::WebDavOptions {
                root: "/remote.php/dav/files/anna".into(),
                preset: "nextcloud".into(),
            }));
            let c = place_chip(&remote("/"), Some((&dav, "x")), None);
            assert_eq!(c.label, "WebDAV · cloud.example.com");
            assert!(
                c.tooltip
                    .contains("https://cloud.example.com/remote.php/dav/files/anna/"),
                "{}",
                c.tooltip
            );

            assert_eq!(
                place_chip(&remote("/"), None, None).label,
                t!("app.disconnected")
            );
        });
    }

    #[test]
    fn cloud_folders_show_the_provider_and_the_account_only_in_the_tooltip() {
        crate::i18n::with_locale(crate::i18n::Locale::De, || {
            let dir = tempfile::tempdir().unwrap();
            for name in ["GoogleDrive-me@example.com", "OneDrive-Personal", "Dropbox"] {
                std::fs::create_dir_all(dir.path().join("Library/CloudStorage").join(name))
                    .unwrap();
            }
            std::fs::create_dir_all(
                dir.path()
                    .join("Library/Mobile Documents/com~apple~CloudDocs"),
            )
            .unwrap();
            let locs = cloud_local::discover_in(dir.path());
            let fs: SharedVfs = Arc::new(manta_vfs::local::LocalFs);
            let chip = |p: PathBuf| {
                let loc = Location::new(fs.clone(), None, VfsPath::new(&p.to_string_lossy()));
                place_chip(&loc, None, cloud_local::location_of(&p, &locs))
            };
            let g = chip(
                dir.path()
                    .join("Library/CloudStorage/GoogleDrive-me@example.com/My Drive"),
            );
            assert_eq!(
                (g.label.as_str(), g.icon, g.local),
                ("Google Drive", icons::GOOGLE_DRIVE_LOGO, false)
            );
            assert!(g.tooltip.contains("me@example.com"));
            assert_eq!(
                chip(dir.path().join("Library/CloudStorage/OneDrive-Personal")).label,
                "OneDrive"
            );
            assert_eq!(
                chip(dir.path().join("Library/CloudStorage/Dropbox")).label,
                "Dropbox"
            );
            let i = chip(
                dir.path()
                    .join("Library/Mobile Documents/com~apple~CloudDocs"),
            );
            assert_eq!(i.label, "iCloud Drive");
        });
    }

    #[test]
    fn archives_show_their_name() {
        crate::i18n::with_locale(crate::i18n::Locale::De, || {
            let fs: SharedVfs = Arc::new(manta_vfs::local::LocalFs);
            let outer = Location::new(fs.clone(), None, VfsPath::new("/Users/me"));
            let mut loc = Location::new(fs, None, VfsPath::root());
            loc.archive = Some(Arc::new(crate::pane::ArchiveMount {
                outer,
                file_name: "backup.zip".into(),
                format: "zip".into(),
            }));
            let c = place_chip(&loc, None, None);
            assert_eq!(
                (c.label.as_str(), c.icon, c.local),
                ("Archiv · backup.zip", icons::ARCHIVE, false)
            );
        });
    }
}
