//! Mounted volumes (`getmntinfo`): which ones are network shares (SMB,
//! AFP, NFS, WebDAV mounted by Finder or Manta), and ejecting them.

use std::path::{Path, PathBuf};

use manta_core::{VfsError, VfsResult};

/// One mounted file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// Where it is mounted (`/Volumes/Share`).
    pub path: PathBuf,
    /// File system type (`apfs`, `smbfs`, `afpfs`, `nfs`, `webdav`, ...).
    pub fs_type: String,
    /// What is mounted (`//alice@nas.local/Share`, `/dev/disk3s1`, ...).
    pub from: String,
}

impl Volume {
    /// A network share (SMB, AFP, NFS, WebDAV, FTP mounted by Finder).
    pub fn is_network(&self) -> bool {
        is_network_type(&self.fs_type)
    }

    /// For SMB / AFP: `(server, share)` from `//[domain;][user@]server/share`.
    pub fn server_share(&self) -> Option<(String, String)> {
        parse_from(&self.from)
    }
}

/// Network file system types.
pub fn is_network_type(t: &str) -> bool {
    matches!(
        t,
        "smbfs" | "afpfs" | "nfs" | "webdav" | "ftp" | "cifs" | "smb"
    )
}

/// `(server, share)` of an SMB / AFP mount source such as
/// `//WORKGROUP;alice@nas.local/My%20Share`.
pub fn parse_from(from: &str) -> Option<(String, String)> {
    let rest = from.strip_prefix("//").or_else(|| {
        // afp://user@server/share style.
        from.find("://").map(|i| &from[i + 3..])
    })?;
    let (auth_host, share) = rest.split_once('/')?;
    let host = auth_host.rsplit('@').next().unwrap_or(auth_host);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() || share.is_empty() {
        return None;
    }
    Some((
        crate::webdav::decode(host),
        crate::webdav::decode(share.trim_end_matches('/')),
    ))
}

/// All mounted file systems.
#[cfg(target_os = "macos")]
pub fn mounted() -> Vec<Volume> {
    use std::ffi::CStr;
    let mut buf: *mut libc::statfs = std::ptr::null_mut();
    // SAFETY: getmntinfo fills `buf` with a pointer to `n` statfs records
    // in a buffer it owns (valid until the next call on this thread).
    let n = unsafe { libc::getmntinfo(&mut buf, libc::MNT_NOWAIT) };
    if n <= 0 || buf.is_null() {
        return Vec::new();
    }
    // SAFETY: see above; `n` records are valid.
    let records = unsafe { std::slice::from_raw_parts(buf, n as usize) };
    let text = |c: &[libc::c_char]| {
        // SAFETY: the fields are NUL-terminated C strings.
        unsafe { CStr::from_ptr(c.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    };
    records
        .iter()
        .map(|r| Volume {
            path: PathBuf::from(text(&r.f_mntonname)),
            fs_type: text(&r.f_fstypename),
            from: text(&r.f_mntfromname),
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub fn mounted() -> Vec<Volume> {
    Vec::new()
}

/// The mounted network volumes.
pub fn network_volumes() -> Vec<Volume> {
    mounted().into_iter().filter(Volume::is_network).collect()
}

/// Unmount (eject) the volume at `path` with `diskutil unmount`, like
/// Finder's eject button. Blocking; call off the UI thread.
pub fn eject(path: &Path) -> VfsResult<()> {
    let out = std::process::Command::new("/usr/sbin/diskutil")
        .arg("unmount")
        .arg(path)
        .output()
        .map_err(|e| VfsError::Other(format!("diskutil: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let msg = if msg.is_empty() {
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    } else {
        msg
    };
    if msg.to_ascii_lowercase().contains("in use") || msg.contains("dissent") {
        Err(VfsError::Other(format!(
            "{}: the volume is in use: {msg}",
            path.display()
        )))
    } else {
        Err(VfsError::Other(format!("{}: {msg}", path.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_sources() {
        assert_eq!(
            parse_from("//alice@nas.local/Share"),
            Some(("nas.local".into(), "Share".into()))
        );
        assert_eq!(
            parse_from("//WORKGROUP;bob@10.0.0.5/My%20Files"),
            Some(("10.0.0.5".into(), "My Files".into()))
        );
        assert_eq!(
            parse_from("//GUEST:@server/pub"),
            Some(("server".into(), "pub".into()))
        );
        assert_eq!(
            parse_from("afp://me@mac.local/Data"),
            Some(("mac.local".into(), "Data".into()))
        );
        assert_eq!(parse_from("/dev/disk3s1"), None);
        assert!(is_network_type("smbfs") && !is_network_type("apfs"));
    }

    #[test]
    fn root_is_mounted() {
        if cfg!(target_os = "macos") {
            let all = mounted();
            assert!(all.iter().any(|v| v.path == Path::new("/")), "{all:?}");
        }
    }
}
