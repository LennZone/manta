//! Streaming, resumable download of a release ZIP with size, SHA-256 and
//! Ed25519 verification.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ed25519_dalek::VerifyingKey;
use sha2::{Digest, Sha256};

use crate::check::{agent, net_error, validate_url};
use crate::error::{Result, UpdateError};
use crate::keys;
use crate::manifest::{Release, DEFAULT_APP_ID, MAX_RELEASE_SIZE};

/// A ZIP whose size, SHA-256 and signature matched its release. Only this
/// module creates it, so [`crate::prepare`] can't be called on unchecked data.
#[derive(Clone, Debug)]
pub struct VerifiedDownload {
    path: PathBuf,
    release: Release,
}

impl VerifiedDownload {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn release(&self) -> &Release {
        &self.release
    }
}

/// Downloads with the built-in key (see [`download_with`]).
pub fn download(
    release: &Release,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<VerifiedDownload> {
    download_with(
        &keys::embedded_public_key()?,
        DEFAULT_APP_ID,
        release,
        dest_dir,
        progress,
        cancel,
    )
}

/// Downloads `release.url` into `dest_dir/Manta-<version>.zip`.
///
/// * The release signature is checked first (nothing is downloaded for a forged entry).
/// * Data goes to `<name>.part`; a later call resumes it with an HTTP `Range`
///   request when the server supports it (otherwise it starts over).
/// * `progress(done, total)` is called as data arrives; `cancel` is polled per chunk
///   (a cancelled download keeps its `.part` file for resuming).
/// * On success the file is renamed to its final name; on a size or hash
///   mismatch the partial data is deleted.
pub fn download_with(
    key: &VerifyingKey,
    app_id: &str,
    release: &Release,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<VerifiedDownload> {
    keys::verify_release(key, app_id, release)?;
    validate_url(&release.url)?;
    if release.size > MAX_RELEASE_SIZE {
        return Err(UpdateError::Integrity(format!(
            "implausible size {}",
            release.size
        )));
    }
    fs::create_dir_all(dest_dir).map_err(|e| UpdateError::io(dest_dir.display(), e))?;
    let final_path = dest_dir.join(release.file_name());
    let part_path = dest_dir.join(format!("{}.part", release.file_name()));
    let total = release.size;

    // Already downloaded earlier?
    if final_path.is_file() {
        if let Ok(v) = verify_file(key, app_id, release, &final_path) {
            progress(total, total);
            return Ok(v);
        }
        let _ = fs::remove_file(&final_path);
    }

    let mut have = fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
    if have >= total {
        let _ = fs::remove_file(&part_path);
        have = 0;
    }

    let agent = agent(&release.url, None);
    let mut req = agent.get(&release.url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let mut resp = req.call().map_err(|e| net_error(&release.url, e))?;
    let status = resp.status().as_u16();
    let resumed = match status {
        206 if have > 0 => {
            // Content-Range: bytes <start>-<end>/<total>
            let start = resp
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes "))
                .and_then(|v| v.split('-').next())
                .and_then(|v| v.trim().parse::<u64>().ok());
            if start != Some(have) {
                return Err(UpdateError::Network(
                    "the server sent an unexpected byte range; try again".into(),
                ));
            }
            true
        }
        200 => false,
        416 => {
            let _ = fs::remove_file(&part_path);
            return Err(UpdateError::Network(
                "the server refused to resume the download; try again".into(),
            ));
        }
        _ => {
            return Err(UpdateError::Http {
                status,
                url: release.url.clone(),
            })
        }
    };

    let mut hasher = Sha256::new();
    let mut file = if resumed {
        let mut f = OpenOptions::new()
            .read(true)
            .append(true)
            .open(&part_path)
            .map_err(|e| UpdateError::io(part_path.display(), e))?;
        f.seek(SeekFrom::Start(0))
            .map_err(|e| UpdateError::io(part_path.display(), e))?;
        hash_reader(&mut (&f).take(have), &mut hasher)
            .map_err(|e| UpdateError::io(part_path.display(), e))?;
        f.seek(SeekFrom::End(0))
            .map_err(|e| UpdateError::io(part_path.display(), e))?;
        f
    } else {
        have = 0;
        File::create(&part_path).map_err(|e| UpdateError::io(part_path.display(), e))?
    };

    progress(have, total);
    let mut reader = resp.body_mut().as_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = have;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = file.flush();
            return Err(UpdateError::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(UpdateError::Network(format!(
                    "download interrupted ({e}); it resumes next time"
                )))
            }
        };
        done += n as u64;
        if done > total {
            drop(file);
            let _ = fs::remove_file(&part_path);
            return Err(UpdateError::Integrity(format!(
                "the server sent more than the expected {total} bytes"
            )));
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| UpdateError::io(part_path.display(), e))?;
        progress(done, total);
    }
    file.sync_all()
        .map_err(|e| UpdateError::io(part_path.display(), e))?;
    drop(file);
    if done != total {
        return Err(UpdateError::Network(format!(
            "the download ended early ({done} of {total} bytes); it resumes next time"
        )));
    }
    let digest = hex::encode(hasher.finalize());
    if digest != release.sha256.to_ascii_lowercase() {
        let _ = fs::remove_file(&part_path);
        return Err(UpdateError::Integrity(format!(
            "SHA-256 is {digest}, expected {}",
            release.sha256
        )));
    }
    fs::rename(&part_path, &final_path).map_err(|e| UpdateError::io(final_path.display(), e))?;
    Ok(VerifiedDownload {
        path: final_path,
        release: release.clone(),
    })
}

fn hash_reader(r: &mut dyn Read, hasher: &mut Sha256) -> std::io::Result<u64> {
    let mut buf = vec![0u8; 64 * 1024];
    let mut n_total = 0;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(n_total);
        }
        hasher.update(&buf[..n]);
        n_total += n as u64;
    }
}

/// SHA-256 (lowercase hex) and size of a file.
pub fn sha256_file(path: &Path) -> std::io::Result<(String, u64)> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let n = hash_reader(&mut f, &mut hasher)?;
    Ok((hex::encode(hasher.finalize()), n))
}

/// Re-verifies a ZIP on disk against its release (e.g. a download kept from
/// an earlier session in [`crate::UpdateState::downloaded`]).
pub fn verify_file(
    key: &VerifyingKey,
    app_id: &str,
    release: &Release,
    path: &Path,
) -> Result<VerifiedDownload> {
    keys::verify_release(key, app_id, release)?;
    let (digest, size) = sha256_file(path).map_err(|e| UpdateError::io(path.display(), e))?;
    if size != release.size {
        return Err(UpdateError::Integrity(format!(
            "size is {size} bytes, expected {}",
            release.size
        )));
    }
    if digest != release.sha256.to_ascii_lowercase() {
        return Err(UpdateError::Integrity(format!(
            "SHA-256 is {digest}, expected {}",
            release.sha256
        )));
    }
    Ok(VerifiedDownload {
        path: path.to_path_buf(),
        release: release.clone(),
    })
}
