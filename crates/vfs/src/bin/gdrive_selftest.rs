//! `manta-gdrive-selftest`: checks the Google Drive backend against a real
//! account once an OAuth client is set up (docs/cloud.md).
//!
//! ```text
//! MANTA_GDRIVE_CLIENT_ID=… MANTA_GDRIVE_CLIENT_SECRET=… \
//!   cargo run -p manta-vfs --bin manta-gdrive-selftest -- --sign-in
//! cargo run -p manta-vfs --bin manta-gdrive-selftest -- --email you@gmail.com
//! ```
//!
//! `--sign-in` opens the browser and stores the sign-in in the Keychain
//! (like the app); `--email` reuses a stored sign-in. The test works in a new
//! folder "Manta self-test <time>" in My Drive and moves it to the Drive
//! trash at the end (`--keep` leaves it).

use std::io::{Read, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use manta_core::{Vfs, VfsPath, VfsResult};
use manta_vfs::gdrive::{self, Area, AuthCancel, GDriveFs, KeychainStore, OAuthClient, TokenStore};

fn step<T>(name: &str, f: impl FnOnce() -> VfsResult<T>) -> Option<T> {
    let t = Instant::now();
    match f() {
        Ok(v) => {
            println!("  ok    {name} ({} ms)", t.elapsed().as_millis());
            Some(v)
        }
        Err(e) => {
            println!("  FAIL  {name}: {e}");
            None
        }
    }
}

fn noise(len: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let Some(client) = OAuthClient::configured() else {
        eprintln!("{}.", gdrive::NOT_CONFIGURED);
        eprintln!("Set MANTA_GDRIVE_CLIENT_ID and MANTA_GDRIVE_CLIENT_SECRET (see docs/cloud.md).");
        return ExitCode::from(2);
    };
    println!(
        "OAuth client: {} (from {:?})",
        client.client_id, client.source
    );

    let email = if flag("--sign-in") {
        println!("Opening the browser for the Google sign-in (5 minutes) …");
        let open = |url: &str| {
            if std::process::Command::new("open")
                .arg(url)
                .status()
                .is_err()
            {
                println!("Open this URL: {url}");
            }
        };
        match gdrive::sign_in(open, &AuthCancel::new()) {
            Ok(cfg) => {
                println!(
                    "Signed in as {} (refresh token stored in the Keychain)",
                    cfg.username
                );
                cfg.username
            }
            Err(e) => {
                eprintln!("Sign-in failed: {e}");
                return ExitCode::from(1);
            }
        }
    } else if let Some(e) = value("--email") {
        e
    } else {
        eprintln!("usage: manta-gdrive-selftest (--sign-in | --email you@gmail.com) [--keep] [--sign-out]");
        return ExitCode::from(2);
    };

    let store: Arc<dyn TokenStore> = Arc::new(KeychainStore);
    let Some(fs) = step("connect", || GDriveFs::open(&email, client.clone(), store)) else {
        return ExitCode::from(1);
    };
    let mut failed = false;
    let mut check = |ok: bool| failed |= !ok;

    check(step("list /", || fs.list(&VfsPath::root())).is_some());
    let my = Area::MyDrive.path();
    let entries = step("list My Drive", || fs.list(&my));
    check(entries.is_some());
    if let Some(entries) = &entries {
        for e in entries.iter().take(10) {
            println!("        {:?}  {}  {} bytes", e.kind, e.name, e.size);
        }
        if let Some(doc) = entries
            .iter()
            .find(|e| e.permissions == Some(0o444) && e.name.ends_with(".docx"))
        {
            check(
                step(&format!("export {}", doc.name), || {
                    let mut v = Vec::new();
                    fs.open_read(&doc.path)?.read_to_end(&mut v)?;
                    Ok(v.len())
                })
                .is_some(),
            );
        }
    }
    check(
        step("list Shared with me", || {
            fs.list(&Area::SharedWithMe.path())
        })
        .is_some(),
    );
    check(step("list Shared drives", || fs.list(&Area::SharedDrives.path())).is_some());

    let dir = my.join(&format!(
        "Manta self-test {}",
        chrono::Local::now().format("%Y-%m-%d %H.%M.%S")
    ));
    if step("create folder", || fs.create_dir(&dir)).is_none() {
        return ExitCode::from(1);
    }
    // Two chunks plus a tail: exercises the resumable upload.
    let data = noise(gdrive::UPLOAD_CHUNK * 2 + 123_457);
    let file = dir.join("upload.bin");
    check(
        step(&format!("upload {} MB", data.len() / 1_000_000), || {
            let mut w = fs.open_write(&file)?;
            w.write_all(&data)?;
            w.flush()?;
            Ok(())
        })
        .is_some(),
    );
    check(
        step("download and compare", || {
            let mut v = Vec::new();
            fs.open_read(&file)?.read_to_end(&mut v)?;
            if v == data {
                Ok(())
            } else {
                Err(manta_core::VfsError::Other(format!(
                    "content differs ({} vs {} bytes)",
                    v.len(),
                    data.len()
                )))
            }
        })
        .is_some(),
    );
    let renamed = dir.join("renamed.bin");
    check(step("rename", || fs.rename(&file, &renamed)).is_some());
    check(
        step("set modified time", || {
            fs.set_modified(&renamed, chrono::Utc::now() - chrono::Duration::days(3))
        })
        .is_some(),
    );
    check(step("stat", || fs.stat(&renamed)).is_some());
    if !flag("--keep") {
        check(
            step("move the test folder to the Drive trash", || {
                fs.remove_tree(&dir)
            })
            .is_some(),
        );
    }
    if flag("--sign-out") {
        check(step("sign out", || gdrive::sign_out(&email)).is_some());
    }
    if failed {
        println!("Some steps failed.");
        ExitCode::from(1)
    } else {
        println!("All steps passed.");
        ExitCode::SUCCESS
    }
}
