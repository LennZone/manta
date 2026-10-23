//! Errors of the update engine. `Display` texts are meant for the user (the
//! UI shows them as they are), so they say what happened and what to do.

/// Everything that can go wrong while checking, downloading or installing.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// No public key or no feed URL compiled in, or similar configuration gaps.
    #[error("Updates are disabled: {0}")]
    Disabled(String),
    #[error("Refusing the insecure address {0}: updates are only loaded over https:// (http:// only from 127.0.0.1/localhost).")]
    InsecureUrl(String),
    #[error("Invalid update address: {0}")]
    InvalidUrl(String),
    #[error("Could not reach the update server: {0}")]
    Network(String),
    #[error("The update server answered {status} for {url}.")]
    Http { status: u16, url: String },
    #[error("The update feed is invalid: {0}")]
    Manifest(String),
    /// Ed25519 verification failed (feed or release). Never install after this.
    #[error("Signature check failed: {0}")]
    Signature(String),
    /// Size or SHA-256 of the download don't match the signed release.
    #[error("The download is damaged or was tampered with: {0}")]
    Integrity(String),
    #[error("Cancelled.")]
    Cancelled,
    /// The unpacked app is not acceptable (bundle ID, version, code signature).
    #[error("The downloaded app was rejected: {0}")]
    Bundle(String),
    /// The running app can't be replaced where it is (DMG, translocated, `cargo run`).
    #[error("Manta can't update itself here: {0}")]
    Location(String),
    #[error("Installing the update failed: {0}")]
    Install(String),
    #[error("{0}")]
    Io(String),
}

impl UpdateError {
    pub(crate) fn io(context: impl std::fmt::Display, e: std::io::Error) -> UpdateError {
        UpdateError::Io(format!("{context}: {e}"))
    }
}

pub type Result<T, E = UpdateError> = std::result::Result<T, E>;
