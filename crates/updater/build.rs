//! Embeds the update-signing public key and the default feed URL at build time.
//!
//! Public key (first match wins):
//!   1. env `MANTA_UPDATE_PUBKEY` (base64 of the 32-byte Ed25519 key)
//!   2. `packaging/update-public-key.txt` (created by `manta-update-keygen`)
//!
//! Without a key the updater compiles fine but reports "updates disabled".
//! Feed URL: env `MANTA_UPDATE_URL`, else [`DEFAULT_FEED_URL`] (the app's
//! Info.plist key `MantaUpdateFeedURL` takes precedence at run time, see
//! docs/updates.md). Set `MANTA_UPDATE_URL=` (empty) to compile without a feed.
//! The pre-rename name `RUSTLIFT_UPDATE_URL` is still accepted (deprecation
//! alias, with a build warning).
use std::path::PathBuf;

/// The feed the official builds look at.
const DEFAULT_FEED_URL: &str = "https://manta-filemanager.com/updates/update.json";

/// `MANTA_<name>`, or the deprecated `RUSTLIFT_<name>` alias with a warning.
fn env_var(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed=MANTA_{name}");
    println!("cargo:rerun-if-env-changed=RUSTLIFT_{name}");
    if let Ok(v) = std::env::var(format!("MANTA_{name}")) {
        return Some(v);
    }
    let old = std::env::var(format!("RUSTLIFT_{name}")).ok()?;
    println!("cargo:warning=RUSTLIFT_{name} is deprecated, use MANTA_{name}");
    Some(old)
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let packaging = manifest_dir.join("../../packaging");
    let key_file = packaging.join("update-public-key.txt");
    println!("cargo:rerun-if-changed={}", key_file.display());
    // The directory's mtime changes when the key file is created.
    println!("cargo:rerun-if-changed={}", packaging.display());

    let from_env = env_var("UPDATE_PUBKEY").filter(|s| !s.trim().is_empty());
    let key = from_env
        .or_else(|| std::fs::read_to_string(&key_file).ok())
        .map(|text| {
            // Ignore comment lines, keep the first non-empty line.
            text.lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
                .unwrap_or("")
                .to_string()
        });
    println!(
        "cargo:rustc-env=MANTA_UPDATE_PUBKEY_EMBEDDED={}",
        key.unwrap_or_default()
    );
    let url = env_var("UPDATE_URL").unwrap_or_else(|| DEFAULT_FEED_URL.to_string());
    println!("cargo:rustc-env=MANTA_UPDATE_URL_EMBEDDED={}", url.trim());
}
