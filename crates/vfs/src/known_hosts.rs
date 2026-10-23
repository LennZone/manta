//! SSH host key verification for the SFTP backend ("trust on first use").
//!
//! - A key listed in `known_hosts` that matches: connect.
//! - A host that is not listed: [`VfsError::UnknownHostKey`] with the key's
//!   type and SHA-256 fingerprint. The app asks the user; if they accept, it
//!   calls [`trust_host_key`] (appends the key to `known_hosts`) and connects
//!   again. The backend never blocks on UI.
//! - A host that is listed with a different key: [`VfsError::HostKeyMismatch`].
//!   Manta never connects then; the user has to remove the old line
//!   (`ssh-keygen -R host`) after checking with the server's administrator.
//!
//! The file is `~/.ssh/known_hosts`, unless [`set_known_hosts_file`] or the
//! environment variable `MANTA_KNOWN_HOSTS` points somewhere else (tests).
//! Entries are read line by line, so a line libssh2 can't parse (certificate
//! authorities, unsupported key types) doesn't hide the rest of the file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use base64::Engine as _;
use manta_core::{HostKeyInfo, VfsError, VfsResult};
use sha2::{Digest, Sha256};

/// Environment override of the `known_hosts` file.
pub const KNOWN_HOSTS_ENV: &str = "MANTA_KNOWN_HOSTS";

static OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Keys the user accepted in this session (kept even if `known_hosts`
/// could not be written): (host, port, key blob).
static ACCEPTED: Mutex<Vec<(String, u16, Vec<u8>)>> = Mutex::new(Vec::new());

/// Use `path` instead of `~/.ssh/known_hosts` (tests; `None` restores the
/// default). Process-wide.
pub fn set_known_hosts_file(path: Option<PathBuf>) {
    *OVERRIDE.write().unwrap_or_else(|e| e.into_inner()) = path;
}

/// The `known_hosts` file that is checked and appended to.
pub fn known_hosts_file() -> Option<PathBuf> {
    if let Some(p) = OVERRIDE.read().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(p);
    }
    if let Some(p) = std::env::var_os(KNOWN_HOSTS_ENV).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts"))
}

/// OpenSSH fingerprint of a public key blob: `SHA256:<unpadded base64>`.
pub fn fingerprint(key: &[u8]) -> String {
    let digest = Sha256::digest(key);
    format!(
        "SHA256:{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest)
    )
}

/// The key type name at the start of an SSH public key blob
/// (`ssh-ed25519`, `ssh-rsa`, `ecdsa-sha2-nistp256`, ...).
pub fn key_type_of(key: &[u8]) -> Option<String> {
    let len = u32::from_be_bytes(key.get(..4)?.try_into().ok()?) as usize;
    let name = key.get(4..4 + len)?;
    let name = std::str::from_utf8(name).ok()?;
    (!name.is_empty() && name.is_ascii()).then(|| name.to_string())
}

/// The host pattern as OpenSSH writes it: `host` for port 22,
/// `[host]:port` otherwise.
pub fn host_pattern(host: &str, port: u16) -> String {
    if port == 22 || port == 0 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

/// Result of looking a host key up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyStatus {
    Known,
    Unknown,
    Changed,
}

/// Build the info for a key presented by `host:port`.
pub fn host_key_info(host: &str, port: u16, key: &[u8], file: Option<&Path>) -> HostKeyInfo {
    HostKeyInfo {
        host: host.to_string(),
        port,
        key_type: key_type_of(key).unwrap_or_else(|| "unknown".into()),
        fingerprint: fingerprint(key),
        key: key.to_vec(),
        known_hosts: file.map(|f| f.display().to_string()).unwrap_or_default(),
    }
}

/// Look `key` up for `host:port` in `file` (missing file: unknown).
pub fn check(file: Option<&Path>, host: &str, port: u16, key: &[u8]) -> VfsResult<HostKeyStatus> {
    use ssh2::{CheckResult, KnownHostFileKind};

    let session = ssh2::Session::new()
        .map_err(|e| VfsError::Connection(format!("cannot create SSH session: {e}")))?;
    let mut kh = session
        .known_hosts()
        .map_err(|e| VfsError::Connection(format!("known_hosts: {e}")))?;
    let text = match file.map(std::fs::read_to_string) {
        Some(Ok(t)) => t,
        Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Some(Err(e)) => {
            // Unreadable: treat as unknown (the user is asked), never as known.
            log::warn!(
                "cannot read {}: {e}",
                file.map(|f| f.display().to_string()).unwrap_or_default()
            );
            String::new()
        }
        None => String::new(),
    };
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with('@') {
            continue;
        }
        // Lines libssh2 can't parse are skipped, not fatal.
        let _ = kh.read_str(l, KnownHostFileKind::OpenSSH);
    }
    let status = match kh.check_port(host, port, key) {
        CheckResult::Match => HostKeyStatus::Known,
        CheckResult::Mismatch => HostKeyStatus::Changed,
        CheckResult::NotFound | CheckResult::Failure => HostKeyStatus::Unknown,
    };
    if status == HostKeyStatus::Unknown {
        let accepted = ACCEPTED.lock().unwrap_or_else(|e| e.into_inner());
        if accepted
            .iter()
            .any(|(h, p, k)| h == host && *p == port && k == key)
        {
            return Ok(HostKeyStatus::Known);
        }
    }
    Ok(status)
}

/// Check the key a server presented; `Ok` only if it is known and matches.
pub fn verify(host: &str, port: u16, key: &[u8]) -> VfsResult<()> {
    let file = known_hosts_file();
    match check(file.as_deref(), host, port, key)? {
        HostKeyStatus::Known => Ok(()),
        HostKeyStatus::Unknown => Err(VfsError::UnknownHostKey(Box::new(host_key_info(
            host,
            port,
            key,
            file.as_deref(),
        )))),
        HostKeyStatus::Changed => {
            log::warn!(
                "host key for {} does not match {}",
                host_pattern(host, port),
                file.as_deref()
                    .map(|f| f.display().to_string())
                    .unwrap_or_default()
            );
            Err(VfsError::HostKeyMismatch(Box::new(host_key_info(
                host,
                port,
                key,
                file.as_deref(),
            ))))
        }
    }
}

/// The user accepted an unknown host key: remember it for this session and
/// append it to `known_hosts` (plain host entry, like `ssh` with
/// `HashKnownHosts no`). A key that conflicts with an existing entry is
/// refused. The key is trusted for this session even if writing fails
/// (the error is returned so the app can say so).
pub fn trust_host_key(info: &HostKeyInfo) -> VfsResult<()> {
    let file = known_hosts_file();
    if check(file.as_deref(), &info.host, info.port, &info.key)? == HostKeyStatus::Changed {
        return Err(VfsError::HostKeyMismatch(Box::new(info.clone())));
    }
    {
        let mut accepted = ACCEPTED.lock().unwrap_or_else(|e| e.into_inner());
        if !accepted
            .iter()
            .any(|(h, p, k)| *h == info.host && *p == info.port && *k == info.key)
        {
            accepted.push((info.host.clone(), info.port, info.key.clone()));
        }
    }
    let Some(file) = file else {
        return Err(VfsError::Other(
            "no home directory for ~/.ssh/known_hosts".into(),
        ));
    };
    append_entry(&file, info)
        .map_err(|e| VfsError::Other(format!("cannot write {}: {e}", file.display())))
}

/// The `known_hosts` line for a key.
pub fn known_hosts_line(info: &HostKeyInfo) -> String {
    let key_type = key_type_of(&info.key).unwrap_or_else(|| info.key_type.clone());
    format!(
        "{} {} {}",
        host_pattern(&info.host, info.port),
        key_type,
        base64::engine::general_purpose::STANDARD.encode(&info.key)
    )
}

fn append_entry(file: &Path, info: &HostKeyInfo) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if let Some(dir) = file.parent() {
        if !dir.exists() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
    }
    // Start on a new line if the file doesn't end with one.
    let needs_newline = std::fs::read(file)
        .map(|b| !b.is_empty() && !b.ends_with(b"\n"))
        .unwrap_or(false);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(file)?;
    let mut line = String::new();
    if needs_newline {
        line.push('\n');
    }
    line.push_str(&known_hosts_line(info));
    line.push('\n');
    f.write_all(line.as_bytes())?;
    f.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ed25519 public key blob with the given 32 key bytes.
    fn ed25519(seed: u8) -> Vec<u8> {
        let mut k = Vec::new();
        k.extend_from_slice(&11u32.to_be_bytes());
        k.extend_from_slice(b"ssh-ed25519");
        k.extend_from_slice(&32u32.to_be_bytes());
        k.extend((0..32u8).map(|i| i.wrapping_mul(7).wrapping_add(seed)));
        k
    }

    #[test]
    fn fingerprint_matches_openssh() {
        // `ssh-keygen -lf` of this key prints the same fingerprint.
        let pubkey = "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
        let blob = base64::engine::general_purpose::STANDARD
            .decode(pubkey)
            .unwrap();
        assert_eq!(key_type_of(&blob).as_deref(), Some("ssh-ed25519"));
        assert_eq!(
            fingerprint(&blob),
            "SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU"
        );
    }

    #[test]
    fn key_type_parsing() {
        assert_eq!(key_type_of(&ed25519(1)).as_deref(), Some("ssh-ed25519"));
        assert_eq!(key_type_of(&[0, 0, 0, 9, b'a']), None, "truncated");
        assert_eq!(key_type_of(&[]), None);
    }

    #[test]
    fn host_patterns() {
        assert_eq!(host_pattern("example.com", 22), "example.com");
        assert_eq!(host_pattern("example.com", 2222), "[example.com]:2222");
    }

    #[test]
    fn unknown_then_trusted_then_changed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ssh").join("known_hosts");
        let key = ed25519(1);
        let other = ed25519(2);
        let check =
            |host: &str, port: u16, key: &[u8]| check(Some(&file), host, port, key).unwrap();

        // No file yet: unknown.
        assert_eq!(
            check("unit-test.invalid", 2222, &key),
            HostKeyStatus::Unknown
        );

        // Accepting writes `[host]:port type base64` (directory created).
        let info = host_key_info("unit-test.invalid", 2222, &key, Some(&file));
        append_entry(&file, &info).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.starts_with("[unit-test.invalid]:2222 ssh-ed25519 AAAA"),
            "{text}"
        );
        assert!(text.ends_with('\n'));
        assert_eq!(check("unit-test.invalid", 2222, &key), HostKeyStatus::Known);
        // Another port of the same host is a different entry.
        assert_eq!(check("unit-test.invalid", 22, &key), HostKeyStatus::Unknown);
        // A different key for the same host and port: changed.
        assert_eq!(
            check("unit-test.invalid", 2222, &other),
            HostKeyStatus::Changed
        );

        // Port 22 is written without brackets and appended after the first.
        let info22 = host_key_info("unit-test.invalid", 22, &other, Some(&file));
        append_entry(&file, &info22).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(
            text.lines()
                .nth(1)
                .unwrap()
                .starts_with("unit-test.invalid ssh-ed25519 "),
            "{text}"
        );
        assert_eq!(check("unit-test.invalid", 22, &other), HostKeyStatus::Known);
        assert_eq!(check("unit-test.invalid", 22, &key), HostKeyStatus::Changed);
    }

    #[test]
    fn bad_lines_do_not_hide_good_ones() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("known_hosts");
        let key = ed25519(3);
        let info = host_key_info("good.invalid", 22, &key, Some(&file));
        // No trailing newline, a comment, garbage and a CA line first.
        std::fs::write(
            &file,
            format!(
                "# comment\nthis is not a key line\n@cert-authority *.invalid ssh-ed25519 AAAA\n{}",
                known_hosts_line(&host_key_info("first.invalid", 22, &ed25519(4), None))
            ),
        )
        .unwrap();
        append_entry(&file, &info).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.lines().last().unwrap().starts_with("good.invalid "),
            "{text}"
        );
        assert_eq!(
            check(Some(&file), "good.invalid", 22, &key).unwrap(),
            HostKeyStatus::Known
        );
        assert_eq!(
            check(Some(&file), "first.invalid", 22, &ed25519(4)).unwrap(),
            HostKeyStatus::Known
        );
    }

    #[test]
    fn hashed_entries_are_understood() {
        // `ssh-keygen -H` of "hashed.invalid ssh-ed25519 <ed25519(5)>".
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("known_hosts");
        std::fs::write(
            &file,
            "|1|tn3kA9KWZhvjcd/smFGiCoyMrNk=|91YzcQefqcLqx/hWDyAM+MmXLYQ= ssh-ed25519 \
             AAAAC3NzaC1lZDI1NTE5AAAAIAUMExohKC82PURLUllgZ251fIOKkZifpq20u8LJ0Nfe\n",
        )
        .unwrap();
        assert_eq!(
            check(Some(&file), "hashed.invalid", 22, &ed25519(5)).unwrap(),
            HostKeyStatus::Known
        );
        assert_eq!(
            check(Some(&file), "hashed.invalid", 22, &ed25519(6)).unwrap(),
            HostKeyStatus::Changed
        );
        assert_eq!(
            check(Some(&file), "other.invalid", 22, &ed25519(5)).unwrap(),
            HostKeyStatus::Unknown
        );
    }

    #[test]
    fn verify_reports_the_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("known_hosts");
        let key = ed25519(6);
        let info = host_key_info("verify.invalid", 2200, &key, Some(&file));
        assert_eq!(info.key_type, "ssh-ed25519");
        assert_eq!(info.fingerprint, fingerprint(&key));
        assert_eq!(info.host_label(), "[verify.invalid]:2200");
        let e = VfsError::UnknownHostKey(Box::new(info.clone()));
        assert!(!e.is_transient());
        assert!(e.to_string().contains(&info.fingerprint), "{e}");
        let e = VfsError::HostKeyMismatch(Box::new(info));
        assert!(e.to_string().contains("man-in-the-middle"), "{e}");
    }
}
