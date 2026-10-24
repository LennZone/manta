//! Server passwords, key passphrases and S3 secret keys of saved favorites
//! live in the macOS login Keychain, not in `favorites.json`.
//!
//! - One generic password item per server account: service
//!   [`SERVICE`], account [`account`] (`sftp://user@host:22`). Favorites for
//!   different folders of the same account share it.
//! - `favorites.json` only records that a favorite's secret is in the
//!   Keychain (`"keychain": true`).
//! - The secret is read when connecting (on the connect worker thread), never
//!   at launch, so an unsigned build asks for Keychain access at most once
//!   per connection instead of once per favorite at every start.
//! - Tests use [`MemorySecrets`]; nothing here logs or prints a secret.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use manta_core::ConnectionConfig;

/// Keychain service of Manta's server secrets.
pub const SERVICE: &str = "com.manta-filemanager.manta.server";

/// Where secrets are kept. Errors are messages without the secret.
pub trait SecretStore: Send + Sync {
    /// The secret for `account`; `Ok(None)` if there is none.
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    /// Create or replace the secret for `account`.
    fn set(&self, account: &str, secret: &str) -> Result<(), String>;
    /// Remove the secret for `account` (no error if there is none).
    fn delete(&self, account: &str) -> Result<(), String>;
}

pub type SharedSecrets = Arc<dyn SecretStore>;

/// The Keychain account of a server login: `protocol://user@host:port`
/// (the port is the protocol's default when 0). Stable across renames and
/// starting folders.
pub fn account(cfg: &ConnectionConfig) -> String {
    let port = if cfg.port == 0 {
        ConnectionConfig::default_port(cfg.protocol)
    } else {
        cfg.port
    };
    format!(
        "{}://{}@{}:{}",
        cfg.protocol
            .to_string()
            .to_ascii_lowercase()
            .replace(' ', "-"),
        cfg.username,
        cfg.host,
        port
    )
}

/// The macOS login Keychain (generic password items of `service`).
pub struct Keychain {
    service: String,
}

impl Keychain {
    pub fn new(service: &str) -> Self {
        Keychain {
            service: service.to_string(),
        }
    }
}

/// `errSecItemNotFound`.
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(target_os = "macos")]
impl SecretStore for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        match security_framework::passwords::get_generic_password(&self.service, account) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "Keychain item is not UTF-8".to_string()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(format!("Keychain: {e}")),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        security_framework::passwords::set_generic_password(
            &self.service,
            account,
            secret.as_bytes(),
        )
        .map_err(|e| format!("Keychain: {e}"))
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        match security_framework::passwords::delete_generic_password(&self.service, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(format!("Keychain: {e}")),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl SecretStore for Keychain {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Err("no Keychain on this platform".into())
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("no Keychain on this platform".into())
    }
    fn delete(&self, _: &str) -> Result<(), String> {
        Err("no Keychain on this platform".into())
    }
}

/// In-memory store for tests (and a model of the Keychain's behaviour).
#[derive(Default)]
pub struct MemorySecrets {
    items: Mutex<HashMap<String, String>>,
    /// Make `set` fail (a locked / denied Keychain).
    pub fail_writes: AtomicBool,
}

impl MemorySecrets {
    pub fn new() -> Arc<MemorySecrets> {
        Arc::new(MemorySecrets::default())
    }

    /// Accounts with a secret, sorted (tests).
    #[cfg(test)]
    pub fn accounts(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        v.sort();
        v
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self
            .items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account)
            .cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        if self.fail_writes.load(Ordering::Relaxed) {
            return Err("Keychain: write denied (test)".into());
        }
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(account);
        Ok(())
    }
}

/// `cfg` with its secret from `store` if it has none itself (connecting
/// with a favorite whose secret is in the Keychain). Blocking: the system
/// may ask for permission. Runs on a worker thread.
pub fn fill_from(
    store: &dyn SecretStore,
    cfg: ConnectionConfig,
) -> Result<ConnectionConfig, String> {
    if crate::favorites::secret_of(&cfg).is_some()
        || !matches!(
            cfg.auth,
            manta_core::Auth::Password(_) | manta_core::Auth::KeyFile { .. }
        )
    {
        return Ok(cfg);
    }
    let account = account(&cfg);
    match store.get(&account) {
        Ok(Some(secret)) => Ok(crate::favorites::with_secret(&cfg, secret)),
        // A password is needed; a key file may work without passphrase.
        Ok(None) if matches!(cfg.auth, manta_core::Auth::KeyFile { .. }) => Ok(cfg),
        Ok(None) => Err(t!("error.secret_missing").into()),
        Err(e) => Err(t!("error.keychain_read", error = e)),
    }
}

/// The app's store: the login Keychain, or memory in unit tests.
pub fn default_store() -> SharedSecrets {
    if cfg!(test) {
        MemorySecrets::new()
    } else {
        Arc::new(Keychain::new(SERVICE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_core::{Auth, Protocol};

    fn cfg(protocol: Protocol, port: u16) -> ConnectionConfig {
        ConnectionConfig {
            name: "n".into(),
            protocol,
            host: "example.com".into(),
            port,
            username: "deploy".into(),
            auth: Auth::Password("pw".into()),
            initial_path: Some("/var/www".into()),
            s3: None,
            container: None,
            ftp: None,
            webdav: None,
            smb: None,
        }
    }

    #[test]
    fn accounts_are_stable() {
        assert_eq!(
            account(&cfg(Protocol::Sftp, 22)),
            "sftp://deploy@example.com:22"
        );
        assert_eq!(
            account(&cfg(Protocol::Sftp, 0)),
            "sftp://deploy@example.com:22",
            "default port"
        );
        assert_eq!(
            account(&cfg(Protocol::Ftps, 990)),
            "ftps://deploy@example.com:990"
        );
        let mut other = cfg(Protocol::Sftp, 22);
        other.name = "renamed".into();
        other.initial_path = None;
        other.auth = Auth::Password("changed".into());
        assert_eq!(
            account(&other),
            account(&cfg(Protocol::Sftp, 22)),
            "name, folder and secret don't matter"
        );
    }

    #[test]
    fn fill_from_store() {
        let s = MemorySecrets::new();
        let mut c = cfg(Protocol::Sftp, 22);
        c.auth = Auth::Password(String::new());
        assert!(fill_from(&*s, c.clone()).is_err(), "no password anywhere");
        s.set(&account(&c), "from-keychain").unwrap();
        assert_eq!(
            fill_from(&*s, c.clone()).unwrap().auth,
            Auth::Password("from-keychain".into())
        );
        // A typed password wins; agent logins need nothing.
        assert_eq!(
            fill_from(&*s, cfg(Protocol::Sftp, 22)).unwrap().auth,
            Auth::Password("pw".into())
        );
        c.auth = Auth::Agent;
        assert_eq!(fill_from(&*s, c.clone()).unwrap().auth, Auth::Agent);
        c.auth = Auth::KeyFile {
            path: "k".into(),
            passphrase: None,
        };
        assert_eq!(
            fill_from(&*s, c.clone()).unwrap().auth,
            Auth::KeyFile {
                path: "k".into(),
                passphrase: Some("from-keychain".into())
            }
        );
        s.delete(&account(&c)).unwrap();
        assert_eq!(
            fill_from(&*s, c.clone()).unwrap().auth,
            c.auth,
            "key without passphrase"
        );
    }

    #[test]
    fn memory_store() {
        let s = MemorySecrets::new();
        assert_eq!(s.get("a").unwrap(), None);
        s.set("a", "1").unwrap();
        s.set("a", "2").unwrap();
        assert_eq!(s.get("a").unwrap().as_deref(), Some("2"));
        s.delete("a").unwrap();
        s.delete("a").unwrap();
        assert_eq!(s.get("a").unwrap(), None);
        s.fail_writes.store(true, Ordering::Relaxed);
        assert!(s.set("b", "x").is_err());
    }

    /// The real login Keychain, with a throwaway service name. Run with
    /// `cargo test -p manta keychain_round_trip -- --ignored`.
    #[test]
    #[ignore = "touches the login Keychain"]
    fn keychain_round_trip() {
        let service = format!("com.manta-filemanager.manta.test-{}", std::process::id());
        let k = Keychain::new(&service);
        let acc = "sftp://test@manta-keychain-test.invalid:22";
        let _ = k.delete(acc);
        assert_eq!(k.get(acc).unwrap(), None);
        k.set(acc, "first").unwrap();
        k.set(acc, "second").unwrap();
        assert_eq!(k.get(acc).unwrap().as_deref(), Some("second"));
        k.delete(acc).unwrap();
        assert_eq!(k.get(acc).unwrap(), None);
        k.delete(acc).unwrap();
    }
}
