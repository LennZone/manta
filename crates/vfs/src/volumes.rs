//! Mounted volumes (`getmntinfo`): which ones are network shares (SMB,
//! AFP, NFS, WebDAV mounted by Finder or Manta) or ejectable disks (disk
//! images, USB sticks, external drives), and ejecting them.

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

/// A local volume Finder shows an eject button for: a disk image, a USB
/// stick, an external drive (not the Mac's internal volumes). Network
/// shares are not checked here (see [`Volume::is_network`]): asking a share
/// whose server is gone can hang.
#[cfg(target_os = "macos")]
pub fn is_ejectable(path: &Path) -> bool {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{
        NSArray, NSNumber, NSString, NSURLVolumeIsEjectableKey, NSURLVolumeIsInternalKey,
        NSURLVolumeIsRemovableKey, NSURL,
    };
    autoreleasepool(|_| {
        let Some(url) = NSURL::from_file_path(path) else {
            return false;
        };
        // SAFETY: Foundation's constant keys, valid for the process lifetime.
        let (k_eject, k_remove, k_internal) = unsafe {
            (
                NSURLVolumeIsEjectableKey,
                NSURLVolumeIsRemovableKey,
                NSURLVolumeIsInternalKey,
            )
        };
        let keys = NSArray::from_slice(&[k_eject, k_remove, k_internal]);
        let Ok(dict) = url.resourceValuesForKeys_error(&keys) else {
            return false;
        };
        let flag = |k: &NSString| {
            dict.objectForKey(k)
                .and_then(|o| o.downcast::<NSNumber>().ok())
                .map(|n| n.boolValue())
        };
        // Disk images report ejectable and removable but no "internal".
        flag(k_eject) == Some(true)
            || flag(k_remove) == Some(true)
            || flag(k_internal) == Some(false)
    })
}

#[cfg(not(target_os = "macos"))]
pub fn is_ejectable(_path: &Path) -> bool {
    false
}

/// The volume's own icon, like Finder shows it: its custom icon
/// (`.VolumeIcon.icns`, set by most app installer disk images), else the
/// one app at its top level. `None`: the generic drive icon.
pub fn custom_icon(volume: &Path) -> Option<PathBuf> {
    let icns = volume.join(".VolumeIcon.icns");
    if icns.is_file() {
        return Some(icns);
    }
    let mut apps = std::fs::read_dir(volume)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "app") && p.is_dir());
    let app = apps.next()?;
    apps.next().is_none().then_some(app)
}

/// The disk image file mounted at `mount` (`None`: not a disk image).
/// Runs `hdiutil info`; blocking, call off the UI thread.
pub fn disk_image_of(mount: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new("/usr/bin/hdiutil")
        .arg("info")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    image_in_info(&String::from_utf8_lossy(&out.stdout), mount)
}

/// The `image-path` of the block in `hdiutil info`'s output that mounts a
/// volume at `mount` (the last tab-separated field of a `/dev/disk` line).
pub fn image_in_info(info: &str, mount: &Path) -> Option<PathBuf> {
    let mut image: Option<&str> = None;
    for line in info.lines() {
        if line.starts_with("=====") {
            image = None;
        } else if let Some(rest) = line.strip_prefix("image-path") {
            image = rest.split_once(':').map(|(_, p)| p.trim());
        } else if line.starts_with("/dev/disk")
            && line.rsplit('\t').next().map(str::trim) == Some(&*mount.to_string_lossy())
        {
            return image.map(PathBuf::from);
        }
    }
    None
}

/// How [`eject`] takes `path` off the Mac.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EjectVerb {
    /// `diskutil unmount`: a network share.
    Unmount,
    /// `diskutil eject`: the whole disk (a disk image is detached).
    Eject,
}

impl EjectVerb {
    /// A disk (`/dev/disk…`) is ejected, anything else (network shares,
    /// unknown mounts) only unmounted.
    pub fn for_path(volumes: &[Volume], path: &Path) -> Self {
        match volumes.iter().find(|v| v.path == path) {
            Some(v) if !v.is_network() && v.from.starts_with("/dev/disk") => EjectVerb::Eject,
            _ => EjectVerb::Unmount,
        }
    }

    fn arg(self) -> &'static str {
        match self {
            EjectVerb::Unmount => "unmount",
            EjectVerb::Eject => "eject",
        }
    }
}

/// Eject the volume at `path` like Finder's eject button: a network share
/// is unmounted, a disk image, USB stick or external drive is ejected
/// (`diskutil eject`, with all volumes of that disk). Blocking; call off
/// the UI thread.
pub fn eject(path: &Path) -> VfsResult<()> {
    let verb = EjectVerb::for_path(&mounted(), path);
    let out = std::process::Command::new("/usr/sbin/diskutil")
        .arg(verb.arg())
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
    fn eject_verb() {
        let vol = |path: &str, fs_type: &str, from: &str| Volume {
            path: PathBuf::from(path),
            fs_type: fs_type.into(),
            from: from.into(),
        };
        let vols = [
            vol("/Volumes/Installer", "apfs", "/dev/disk25s1"),
            vol("/Volumes/Share", "smbfs", "//alice@nas.local/Share"),
        ];
        let verb = |p: &str| EjectVerb::for_path(&vols, Path::new(p));
        assert_eq!(verb("/Volumes/Installer"), EjectVerb::Eject);
        assert_eq!(verb("/Volumes/Share"), EjectVerb::Unmount);
        // Gone already or never mounted: only `unmount` (never a whole disk).
        assert_eq!(verb("/Volumes/Elsewhere"), EjectVerb::Unmount);
    }

    #[test]
    fn disk_image_of_a_mount() {
        let info = "framework       : 703.0.1
================================================
image-path      : /Users/anna/Downloads/Other.dmg
/dev/disk22\tGUID_partition_scheme\t
/dev/disk22s1\t48465300-0000-11AA-AA11-00306543ECAC\t/Volumes/Other
================================================
image-path      : /Users/anna/Downloads/RustLift 0.1.0.dmg
image-alias     : /Users/anna/Downloads/RustLift 0.1.0.dmg
/dev/disk24\tGUID_partition_scheme\t
/dev/disk24s1\t48465300-0000-11AA-AA11-00306543ECAC\t/Volumes/RustLift
";
        assert_eq!(
            image_in_info(info, Path::new("/Volumes/RustLift")),
            Some(PathBuf::from("/Users/anna/Downloads/RustLift 0.1.0.dmg"))
        );
        assert_eq!(
            image_in_info(info, Path::new("/Volumes/Other")),
            Some(PathBuf::from("/Users/anna/Downloads/Other.dmg"))
        );
        assert_eq!(image_in_info(info, Path::new("/Volumes/USB")), None);
    }

    #[test]
    fn custom_volume_icon() {
        let dir = tempfile::tempdir().unwrap();
        let vol = dir.path();
        assert_eq!(custom_icon(vol), None);
        std::fs::create_dir(vol.join("RustLift.app")).unwrap();
        assert_eq!(custom_icon(vol), Some(vol.join("RustLift.app")));
        // Two apps: which one? The generic icon.
        std::fs::create_dir(vol.join("Uninstall.app")).unwrap();
        assert_eq!(custom_icon(vol), None);
        std::fs::write(vol.join(".VolumeIcon.icns"), b"icns").unwrap();
        assert_eq!(custom_icon(vol), Some(vol.join(".VolumeIcon.icns")));
    }

    #[test]
    fn root_is_mounted() {
        if cfg!(target_os = "macos") {
            let all = mounted();
            assert!(all.iter().any(|v| v.path == Path::new("/")), "{all:?}");
            assert!(!is_ejectable(Path::new("/")));
        }
    }
}
