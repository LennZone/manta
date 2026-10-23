//! Saved favorites (remote connections and local folders), persisted as JSON
//! in `<config dir>/Manta/favorites.json`.
//!
//! Secrets (passwords, key passphrases, S3 secret keys) never go into the
//! file: with "Remember password" they are stored in the macOS Keychain
//! (see [`crate::secrets`]) and the favorite only carries `"keychain": true`.
//! Files written by older versions may still hold plain-text secrets; they
//! are moved to the Keychain on load ([`Favorites::move_secrets_to`]).

use std::path::{Path, PathBuf};

use manta_core::{Auth, ConnectionConfig};

use crate::secrets::SecretStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Favorite {
    Connection {
        config: ConnectionConfig,
        /// The secret (password / passphrase / S3 secret key) is in the
        /// Keychain under [`crate::secrets::account`]; `config` has none.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        keychain: bool,
    },
    Local {
        name: String,
        path: String,
    },
}

impl Favorite {
    pub fn display_name(&self) -> String {
        match self {
            Favorite::Connection { config, .. } => {
                if !config.name.trim().is_empty() {
                    config.name.clone()
                } else if config.protocol == manta_core::Protocol::Kubernetes {
                    config
                        .container
                        .as_ref()
                        .map_or_else(|| config.host.clone(), |o| o.pod.clone())
                } else if config.username.is_empty() {
                    config.host.clone()
                } else if config.name.trim().is_empty() {
                    format!("{}@{}", config.username, config.host)
                } else {
                    config.name.clone()
                }
            }
            Favorite::Local { name, .. } => name.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favorites {
    #[serde(default)]
    pub items: Vec<Favorite>,
}

impl Favorites {
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("Manta").join("favorites.json"))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{\"items\":[]}".into())
    }

    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }

    /// Load from disk; a missing file yields an empty list.
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::from_json(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_json()).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn add(&mut self, fav: Favorite) {
        if !self.items.contains(&fav) {
            self.items.push(fav);
        }
    }

    /// The favorite that opens the same place as `fav`, if any (same local
    /// folder, or same server account and starting folder).
    pub fn position_of(&self, fav: &Favorite) -> Option<usize> {
        self.items.iter().position(|f| f.same_place(fav))
    }

    /// Insert `fav` before index `at` (clamped). A favorite for the same
    /// place is not added twice: `Err(index of the existing one)`.
    pub fn insert_at(&mut self, at: usize, fav: Favorite) -> Result<usize, usize> {
        if let Some(i) = self.position_of(&fav) {
            return Err(i);
        }
        let at = at.min(self.items.len());
        self.items.insert(at, fav);
        Ok(at)
    }

    /// Move the favorite at `from` so it lands before the item that was at
    /// `to` (an insertion index in the current list, `len` = the end).
    /// Returns the new index, `None` when nothing moved.
    pub fn move_item(&mut self, from: usize, to: usize) -> Option<usize> {
        if from >= self.items.len() {
            return None;
        }
        let to = to.min(self.items.len());
        // Dropping right before or after itself changes nothing.
        if to == from || to == from + 1 {
            return None;
        }
        let item = self.items.remove(from);
        let dest = if to > from { to - 1 } else { to };
        self.items.insert(dest, item);
        Some(dest)
    }
}

/// `/a/b/` and `/a/b` are the same folder.
fn norm_path(p: &str) -> &str {
    let t = p.trim_end_matches('/');
    if t.is_empty() {
        "/"
    } else {
        t
    }
}

impl Favorite {
    /// Whether both favorites open the same place (names don't matter).
    pub fn same_place(&self, other: &Favorite) -> bool {
        match (self, other) {
            (Favorite::Local { path: a, .. }, Favorite::Local { path: b, .. }) => {
                norm_path(a) == norm_path(b)
            }
            (Favorite::Connection { config: a, .. }, Favorite::Connection { config: b, .. }) => {
                a.protocol == b.protocol
                    && a.host == b.host
                    && a.port == b.port
                    && a.username == b.username
                    && a.container == b.container
                    && a.s3.as_ref().map(|s| &s.bucket) == b.s3.as_ref().map(|s| &s.bucket)
                    && a.initial_path.as_deref().map(norm_path)
                        == b.initial_path.as_deref().map(norm_path)
            }
            _ => false,
        }
    }
}

/// Result of [`Favorites::move_secrets_to`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SecretMigration {
    /// Secrets now in the store (removed from the favorites).
    pub moved: usize,
    /// Secrets the store refused (left in the favorites).
    pub failed: usize,
}

impl Favorites {
    /// Move plain-text secrets (files from older versions) into `store`.
    /// A favorite whose secret was stored loses it and gets the Keychain
    /// marker; one whose secret couldn't be stored keeps it unchanged.
    /// Logs never contain the secret.
    pub fn move_secrets_to(&mut self, store: &dyn SecretStore) -> SecretMigration {
        let mut r = SecretMigration::default();
        for fav in &mut self.items {
            let Favorite::Connection { config, keychain } = fav else {
                continue;
            };
            let Some(secret) = secret_of(config) else {
                continue;
            };
            let account = crate::secrets::account(config);
            match store.set(&account, secret) {
                Ok(()) => {
                    *config = strip_secrets(config);
                    *keychain = true;
                    r.moved += 1;
                }
                Err(e) => {
                    log::warn!(
                        "favorites: cannot move the secret of {account} to the Keychain: {e}"
                    );
                    r.failed += 1;
                }
            }
        }
        r
    }

    /// Whether a favorite other than `except` keeps its secret in the
    /// Keychain under `account` (then the item must stay).
    pub fn keychain_in_use(&self, account: &str, except: Option<usize>) -> bool {
        self.items.iter().enumerate().any(|(i, f)| {
            Some(i) != except
                && matches!(f, Favorite::Connection { config, keychain: true } if crate::secrets::account(config) == account)
        })
    }
}

/// Move plain-text secrets of a just loaded `favorites.json` (older
/// versions) to `store` and rewrite the file without them. If any secret
/// can't be stored, the file is left unchanged (moved ones are in the
/// store already and are moved again next time; harmless).
pub fn migrate_secrets(
    favs: &mut Favorites,
    path: &Path,
    store: &dyn SecretStore,
) -> SecretMigration {
    let r = favs.move_secrets_to(store);
    if r.failed > 0 {
        log::warn!(
            "favorites: {} secret(s) could not be moved to the Keychain; {} left unchanged",
            r.failed,
            path.display()
        );
    } else if r.moved > 0 {
        match favs.save(path) {
            Ok(()) => log::info!(
                "favorites: moved {} secret(s) from {} to the Keychain",
                r.moved,
                path.display()
            ),
            Err(e) => log::warn!("favorites: cannot rewrite {}: {e}", path.display()),
        }
    }
    r
}

/// The secret a config carries (non-empty password / passphrase), if any.
pub fn secret_of(cfg: &ConnectionConfig) -> Option<&str> {
    match &cfg.auth {
        Auth::Password(p) if !p.is_empty() => Some(p),
        Auth::KeyFile {
            passphrase: Some(p),
            ..
        } if !p.is_empty() => Some(p),
        _ => None,
    }
}

/// `cfg` with `secret` filled in where it has none (password, or the key
/// file's passphrase).
pub fn with_secret(cfg: &ConnectionConfig, secret: String) -> ConnectionConfig {
    let mut c = cfg.clone();
    match &mut c.auth {
        Auth::Password(p) if p.is_empty() => *p = secret,
        Auth::KeyFile { passphrase, .. } if passphrase.as_deref().is_none_or(str::is_empty) => {
            *passphrase = Some(secret)
        }
        _ => {}
    }
    c
}

/// Remove secrets from a config before persisting it (unless "remember").
pub fn strip_secrets(cfg: &ConnectionConfig) -> ConnectionConfig {
    let mut c = cfg.clone();
    c.auth = match &cfg.auth {
        Auth::Password(_) => Auth::Password(String::new()),
        Auth::KeyFile { path, .. } => Auth::KeyFile {
            path: path.clone(),
            passphrase: None,
        },
        other => other.clone(),
    };
    c
}

/// True if connecting with this config needs user input (missing password).
pub fn needs_secret(cfg: &ConnectionConfig) -> bool {
    matches!(&cfg.auth, Auth::Password(p) if p.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_core::Protocol;

    fn sample() -> Favorites {
        Favorites {
            items: vec![
                Favorite::Connection {
                    keychain: false,
                    config: ConnectionConfig {
                        name: "web".into(),
                        protocol: Protocol::Sftp,
                        host: "example.com".into(),
                        port: 22,
                        username: "me".into(),
                        auth: Auth::KeyFile {
                            path: "~/.ssh/id_ed25519".into(),
                            passphrase: None,
                        },
                        initial_path: Some("/var/www".into()),
                        s3: None,
                        container: None,
                        ftp: None,
                    },
                },
                Favorite::Local {
                    name: "Projects".into(),
                    path: "/Users/me/Projects".into(),
                },
            ],
        }
    }

    #[test]
    fn json_round_trip() {
        let favs = sample();
        let json = favs.to_json();
        assert!(json.contains("\"type\": \"local\""));
        assert_eq!(Favorites::from_json(&json).unwrap(), favs);
    }

    #[test]
    fn file_round_trip() {
        let dir = std::env::temp_dir().join(format!("manta-fav-test-{}", std::process::id()));
        let path = dir.join("favorites.json");
        let favs = sample();
        favs.save(&path).unwrap();
        assert_eq!(Favorites::load(&path).unwrap(), favs);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(Favorites::load(&path).unwrap(), Favorites::default());
    }

    #[test]
    fn ftps_data_channel_is_persisted() {
        let mut favs = sample();
        let Favorite::Connection { config, .. } = &mut favs.items[0] else {
            unreachable!()
        };
        config.protocol = Protocol::Ftps;
        config.ftp = Some(Box::new(manta_core::FtpOptions {
            data_channel: manta_core::FtpDataChannel::Require,
        }));
        let json = favs.to_json();
        assert!(json.contains("\"data_channel\": \"require\""), "{json}");
        let back = Favorites::from_json(&json).unwrap();
        assert_eq!(back, favs);
        let Favorite::Connection { config, .. } = &back.items[0] else {
            unreachable!()
        };
        assert_eq!(
            strip_secrets(config).ftp,
            config.ftp,
            "kept when the password is stripped"
        );
        // Favorites from before the option load with the default (Automatic).
        let old = json.replace(
            ",\n        \"ftp\": {\n          \"data_channel\": \"require\"\n        }",
            "",
        );
        assert!(!old.contains("data_channel"), "{old}");
        let Favorite::Connection { config, .. } = &Favorites::from_json(&old).unwrap().items[0]
        else {
            unreachable!()
        };
        assert_eq!(config.ftp, None);
    }

    fn local(name: &str, path: &str) -> Favorite {
        Favorite::Local {
            name: name.into(),
            path: path.into(),
        }
    }

    #[test]
    fn insert_refuses_the_same_place_twice() {
        let mut favs = sample();
        assert_eq!(favs.insert_at(1, local("x", "/tmp/x")), Ok(1));
        assert_eq!(favs.items.len(), 3);
        // Same folder under another name / with a trailing slash.
        assert_eq!(
            favs.insert_at(0, local("Other", "/Users/me/Projects/")),
            Err(2)
        );
        assert_eq!(favs.items.len(), 3);
        // Same server, other folder: a new favorite; same folder: refused.
        let Favorite::Connection { config, .. } = favs.items[0].clone() else {
            unreachable!()
        };
        let mut other = config.clone();
        other.initial_path = Some("/srv".into());
        other.name = "srv".into();
        assert_eq!(
            favs.insert_at(
                99,
                Favorite::Connection {
                    config: other,
                    keychain: false
                }
            ),
            Ok(3)
        );
        let mut same = config;
        same.name = "renamed".into();
        same.initial_path = Some("/var/www/".into());
        assert_eq!(
            favs.insert_at(
                0,
                Favorite::Connection {
                    config: same,
                    keychain: false
                }
            ),
            Err(0)
        );
    }

    #[test]
    fn move_item_reorders() {
        let mut favs = Favorites {
            items: vec![local("a", "/a"), local("b", "/b"), local("c", "/c")],
        };
        let names = |f: &Favorites| {
            f.items
                .iter()
                .map(|x| x.display_name())
                .collect::<Vec<_>>()
                .join("")
        };
        assert_eq!(favs.move_item(0, 3), Some(2));
        assert_eq!(names(&favs), "bca");
        assert_eq!(favs.move_item(2, 0), Some(0));
        assert_eq!(names(&favs), "abc");
        assert_eq!(favs.move_item(1, 1), None, "before itself");
        assert_eq!(favs.move_item(1, 2), None, "after itself");
        assert_eq!(favs.move_item(5, 0), None);
        assert_eq!(favs.move_item(2, 1), Some(1));
        assert_eq!(names(&favs), "acb");
    }

    #[test]
    fn secrets_are_stripped() {
        let cfg = ConnectionConfig {
            name: String::new(),
            protocol: Protocol::Ftp,
            host: "h".into(),
            port: 21,
            username: "u".into(),
            auth: Auth::Password("hunter2".into()),
            initial_path: None,
            s3: None,
            container: None,
            ftp: None,
        };
        let s = strip_secrets(&cfg);
        assert_eq!(s.auth, Auth::Password(String::new()));
        assert!(needs_secret(&s));
        assert!(!needs_secret(&cfg));
        assert_ne!(s, cfg);
    }

    fn server(host: &str, auth: Auth) -> Favorite {
        Favorite::Connection {
            keychain: false,
            config: ConnectionConfig {
                name: String::new(),
                protocol: Protocol::Sftp,
                host: host.into(),
                port: 22,
                username: "u".into(),
                auth,
                initial_path: None,
                s3: None,
                container: None,
                ftp: None,
            },
        }
    }

    /// A favorites.json from before the Keychain: secrets in plain text.
    const LEGACY: &str = r#"{
  "items": [
    { "type": "connection", "config": { "name": "a", "protocol": "Sftp", "host": "a.example", "port": 22, "username": "u", "auth": { "Password": "hunter2" }, "initial_path": null } },
    { "type": "connection", "config": { "name": "b", "protocol": "Sftp", "host": "b.example", "port": 22, "username": "u", "auth": { "KeyFile": { "path": "~/.ssh/id", "passphrase": "open sesame" } }, "initial_path": null } },
    { "type": "connection", "config": { "name": "s3", "protocol": "S3", "host": "s3.amazonaws.com", "port": 443, "username": "AKIAEXAMPLE", "auth": { "Password": "s3-secret-key" }, "initial_path": null, "s3": { "provider": "aws", "endpoint": "", "region": "eu-central-1", "bucket": "b", "path_style": false, "profile": null, "account_id": "" } } },
    { "type": "connection", "config": { "name": "c", "protocol": "Sftp", "host": "c.example", "port": 22, "username": "u", "auth": { "Password": "" }, "initial_path": null } },
    { "type": "local", "name": "Home", "path": "/Users/me" }
  ]
}"#;

    #[test]
    fn legacy_secrets_move_to_the_store() {
        let store = crate::secrets::MemorySecrets::new();
        let mut favs = Favorites::from_json(LEGACY).unwrap();
        let r = favs.move_secrets_to(&*store);
        assert_eq!(
            r,
            SecretMigration {
                moved: 3,
                failed: 0
            }
        );
        assert_eq!(
            store.accounts(),
            vec![
                "s3://AKIAEXAMPLE@s3.amazonaws.com:443",
                "sftp://u@a.example:22",
                "sftp://u@b.example:22"
            ]
        );
        assert_eq!(
            store.get("sftp://u@a.example:22").unwrap().as_deref(),
            Some("hunter2")
        );
        assert_eq!(
            store.get("sftp://u@b.example:22").unwrap().as_deref(),
            Some("open sesame")
        );
        assert_eq!(
            store
                .get("s3://AKIAEXAMPLE@s3.amazonaws.com:443")
                .unwrap()
                .as_deref(),
            Some("s3-secret-key")
        );
        let json = favs.to_json();
        for secret in ["hunter2", "open sesame", "s3-secret-key"] {
            assert!(!json.contains(secret), "{secret} still in {json}");
        }
        assert_eq!(json.matches("\"keychain\": true").count(), 3, "{json}");
        // The marker survives a round trip; the empty-password one has none.
        let back = Favorites::from_json(&json).unwrap();
        assert_eq!(back, favs);
        let flags: Vec<bool> = back
            .items
            .iter()
            .filter_map(|f| match f {
                Favorite::Connection { keychain, .. } => Some(*keychain),
                _ => None,
            })
            .collect();
        assert_eq!(flags, vec![true, true, true, false]);
        // The key file path stays, only the passphrase goes.
        let Favorite::Connection { config, .. } = &back.items[1] else {
            unreachable!()
        };
        assert_eq!(
            config.auth,
            Auth::KeyFile {
                path: "~/.ssh/id".into(),
                passphrase: None
            }
        );
        // Running again changes nothing.
        let mut again = back.clone();
        assert_eq!(again.move_secrets_to(&*store), SecretMigration::default());
        assert_eq!(again, back);
    }

    #[test]
    fn migration_rewrites_the_file_only_when_all_secrets_moved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("favorites.json");
        std::fs::write(&path, LEGACY).unwrap();

        // Keychain refuses: the file stays byte for byte.
        let denied = crate::secrets::MemorySecrets::new();
        denied
            .fail_writes
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut favs = Favorites::load(&path).unwrap();
        assert_eq!(migrate_secrets(&mut favs, &path, &*denied).failed, 3);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LEGACY);

        // Keychain accepts: rewritten without any secret, still loadable.
        let store = crate::secrets::MemorySecrets::new();
        let mut favs = Favorites::load(&path).unwrap();
        assert_eq!(migrate_secrets(&mut favs, &path, &*store).moved, 3);
        let text = std::fs::read_to_string(&path).unwrap();
        for secret in ["hunter2", "open sesame", "s3-secret-key"] {
            assert!(!text.contains(secret), "{secret} still in the file");
        }
        assert_eq!(Favorites::load(&path).unwrap(), favs);
        assert_eq!(store.accounts().len(), 3);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn failed_store_keeps_the_secret() {
        let store = crate::secrets::MemorySecrets::new();
        store
            .fail_writes
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut favs = Favorites::from_json(LEGACY).unwrap();
        let before = favs.clone();
        assert_eq!(
            favs.move_secrets_to(&*store),
            SecretMigration {
                moved: 0,
                failed: 3
            }
        );
        assert_eq!(favs, before, "nothing changes when the Keychain refuses");
        assert!(store.accounts().is_empty());
    }

    #[test]
    fn secret_helpers() {
        let pw = server("h", Auth::Password("x".into()));
        let Favorite::Connection { config, .. } = &pw else {
            unreachable!()
        };
        assert_eq!(secret_of(config), Some("x"));
        let stripped = strip_secrets(config);
        assert_eq!(secret_of(&stripped), None);
        assert_eq!(
            with_secret(&stripped, "y".into()).auth,
            Auth::Password("y".into())
        );
        assert_eq!(
            with_secret(config, "y".into()).auth,
            Auth::Password("x".into()),
            "an entered secret wins"
        );
        let key = Auth::KeyFile {
            path: "k".into(),
            passphrase: None,
        };
        let Favorite::Connection { config, .. } = server("h", key) else {
            unreachable!()
        };
        assert_eq!(
            with_secret(&config, "p".into()).auth,
            Auth::KeyFile {
                path: "k".into(),
                passphrase: Some("p".into())
            }
        );
        assert_eq!(secret_of(&with_secret(&config, "p".into())), Some("p"));
    }

    #[test]
    fn keychain_items_shared_by_favorites() {
        let mut favs = Favorites {
            items: vec![
                server("h", Auth::Password(String::new())),
                server("h", Auth::Password(String::new())),
            ],
        };
        for f in &mut favs.items {
            if let Favorite::Connection { keychain, .. } = f {
                *keychain = true;
            }
        }
        if let Favorite::Connection { config, .. } = &mut favs.items[1] {
            config.initial_path = Some("/srv".into());
        }
        let acc = "sftp://u@h:22";
        assert!(
            favs.keychain_in_use(acc, Some(0)),
            "the other favorite still uses it"
        );
        assert!(favs.keychain_in_use(acc, None));
        favs.items.remove(1);
        assert!(!favs.keychain_in_use(acc, Some(0)));
        assert!(!favs.keychain_in_use("sftp://u@other:22", None));
    }
}
