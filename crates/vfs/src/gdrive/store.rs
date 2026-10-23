//! Where refresh tokens are kept: the macOS login Keychain (generic
//! password, service [`KEYCHAIN_SERVICE`], account = the Google email).
//! Tests use [`MemoryStore`] and never touch the real Keychain.

use std::collections::HashMap;
use std::sync::Mutex;

use manta_core::{VfsError, VfsResult};

/// Keychain service name of Manta's Google Drive refresh tokens.
pub const KEYCHAIN_SERVICE: &str = "manta-gdrive";

/// Persistent storage of refresh tokens, keyed by account email.
pub trait TokenStore: Send + Sync {
    fn load(&self, email: &str) -> VfsResult<Option<String>>;
    fn save(&self, email: &str, refresh_token: &str) -> VfsResult<()>;
    fn delete(&self, email: &str) -> VfsResult<()>;
}

/// The macOS login Keychain.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeychainStore;

#[cfg(target_os = "macos")]
impl TokenStore for KeychainStore {
    fn load(&self, email: &str) -> VfsResult<Option<String>> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(KEYCHAIN_SERVICE, email) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            // errSecItemNotFound
            Err(e) if e.code() == -25300 => Ok(None),
            Err(e) => Err(VfsError::Auth(format!(
                "cannot read the Google Drive sign-in from the Keychain: {e}"
            ))),
        }
    }

    fn save(&self, email: &str, refresh_token: &str) -> VfsResult<()> {
        security_framework::passwords::set_generic_password(
            KEYCHAIN_SERVICE,
            email,
            refresh_token.as_bytes(),
        )
        .map_err(|e| {
            VfsError::Other(format!(
                "cannot store the Google Drive sign-in in the Keychain: {e}"
            ))
        })
    }

    fn delete(&self, email: &str) -> VfsResult<()> {
        match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, email) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(e) => Err(VfsError::Other(format!(
                "cannot remove the Google Drive sign-in from the Keychain: {e}"
            ))),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl TokenStore for KeychainStore {
    fn load(&self, _email: &str) -> VfsResult<Option<String>> {
        Ok(None)
    }
    fn save(&self, _email: &str, _refresh_token: &str) -> VfsResult<()> {
        Err(VfsError::Unsupported(
            "the Keychain is only available on macOS".into(),
        ))
    }
    fn delete(&self, _email: &str) -> VfsResult<()> {
        Ok(())
    }
}

/// In-memory store (tests, the self-test binary).
#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl MemoryStore {
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }
}

impl TokenStore for MemoryStore {
    fn load(&self, email: &str) -> VfsResult<Option<String>> {
        Ok(self.0.lock().unwrap().get(email).cloned())
    }
    fn save(&self, email: &str, refresh_token: &str) -> VfsResult<()> {
        self.0
            .lock()
            .unwrap()
            .insert(email.to_string(), refresh_token.to_string());
        Ok(())
    }
    fn delete(&self, email: &str) -> VfsResult<()> {
        self.0.lock().unwrap().remove(email);
        Ok(())
    }
}
