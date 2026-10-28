//! Free space for the status bar: the disk a local folder is on, and the
//! iCloud account's quota inside iCloud Drive (like Finder).

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use manta_core::Space;

/// Free space of the disk `path` is on. macOS: the capacity for "important"
/// use, which counts purgeable space as free (Finder's number); else
/// `statvfs`.
pub fn disk_space(path: &Path) -> Option<Space> {
    #[cfg(target_os = "macos")]
    if let Some(s) = native::important_capacity(path) {
        return Some(s);
    }
    statvfs(path)
}

fn statvfs(path: &Path) -> Option<Space> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs fills the zeroed struct; c is a valid C string.
    let st = unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut st) != 0 {
            return None;
        }
        st
    };
    let unit = st.f_frsize as u64;
    Some(Space {
        available: st.f_bavail as u64 * unit,
        total: Some(st.f_blocks as u64 * unit).filter(|t| *t > 0),
    })
}

/// How long a quota answer is reused (`brctl` is a process start).
const QUOTA_TTL: Duration = Duration::from_secs(60);

static QUOTA: Mutex<Option<(Instant, Option<u64>)>> = Mutex::new(None);

/// What is left of the iCloud account's storage (`brctl quota`), cached
/// for a minute. `None` when iCloud doesn't say.
pub fn icloud_quota() -> Option<Space> {
    let mut cache = QUOTA.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = *cache {
        if at.elapsed() < QUOTA_TTL {
            return v.map(|available| Space {
                available,
                total: None,
            });
        }
    }
    let v = std::process::Command::new("/usr/bin/brctl")
        .arg("quota")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_quota(&String::from_utf8_lossy(&o.stdout)));
    *cache = Some((Instant::now(), v));
    v.map(|available| Space {
        available,
        total: None,
    })
}

/// `17520745338 bytes of quota remaining in personal account`.
pub fn parse_quota(out: &str) -> Option<u64> {
    out.lines().find_map(|l| {
        let (n, rest) = l.trim().split_once(' ')?;
        rest.starts_with("bytes of quota remaining")
            .then(|| n.parse().ok())
            .flatten()
    })
}

#[cfg(target_os = "macos")]
mod native {
    use std::path::Path;

    use manta_core::Space;
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{
        NSArray, NSNumber, NSURLVolumeAvailableCapacityForImportantUsageKey,
        NSURLVolumeTotalCapacityKey, NSURL,
    };

    pub fn important_capacity(path: &Path) -> Option<Space> {
        autoreleasepool(|_| {
            let url = NSURL::from_file_path(path)?;
            // SAFETY: Foundation's constant keys, valid for the process lifetime.
            let (k_avail, k_total) = unsafe {
                (
                    NSURLVolumeAvailableCapacityForImportantUsageKey,
                    NSURLVolumeTotalCapacityKey,
                )
            };
            let keys = NSArray::from_slice(&[k_avail, k_total]);
            let dict = url.resourceValuesForKeys_error(&keys).ok()?;
            let num = |k| {
                dict.objectForKey(k)
                    .and_then(|o| o.downcast::<NSNumber>().ok())
                    .map(|n| n.longLongValue())
            };
            let available = num(k_avail).filter(|v| *v >= 0)? as u64;
            let total = num(k_total).filter(|v| *v > 0).map(|v| v as u64);
            Some(Space { available, total })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_output() {
        assert_eq!(
            parse_quota("17520745338 bytes of quota remaining in personal account\n"),
            Some(17_520_745_338)
        );
        assert_eq!(parse_quota("error: not signed in"), None);
        assert_eq!(parse_quota(""), None);
    }

    #[test]
    fn the_temp_disk_has_space() {
        let s = disk_space(&std::env::temp_dir()).expect("space");
        assert!(s.available > 0);
        assert!(s.total.is_some_and(|t| t >= s.available));
    }
}
