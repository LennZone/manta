//! Live Google Drive test; needs an OAuth client and a stored sign-in:
//!
//! ```text
//! export MANTA_GDRIVE_CLIENT_ID=… MANTA_GDRIVE_CLIENT_SECRET=…
//! cargo run -p manta-vfs --bin manta-gdrive-selftest -- --sign-in   # once
//! MANTA_GDRIVE_TEST_EMAIL=you@gmail.com cargo test -p manta-vfs --test gdrive_live -- --ignored
//! ```
//!
//! Works in a new folder in My Drive and moves it to the Drive trash.

use std::io::{Read, Write};
use std::sync::Arc;

use manta_core::Vfs;
use manta_vfs::gdrive::{Area, GDriveFs, KeychainStore, OAuthClient};

#[test]
#[ignore = "needs a Google OAuth client and a signed-in account (MANTA_GDRIVE_TEST_EMAIL)"]
fn live_round_trip() {
    let (Some(client), Ok(email)) = (
        OAuthClient::configured(),
        std::env::var("MANTA_GDRIVE_TEST_EMAIL"),
    ) else {
        eprintln!("skipped: set MANTA_GDRIVE_CLIENT_ID and MANTA_GDRIVE_TEST_EMAIL");
        return;
    };
    let fs = GDriveFs::open(&email, client, Arc::new(KeychainStore)).expect("connect");
    assert_eq!(fs.list(&manta_core::VfsPath::root()).unwrap().len(), 3);
    let dir = Area::MyDrive.path().join(&format!(
        "Manta live test {}",
        chrono::Utc::now().timestamp()
    ));
    fs.create_dir(&dir).unwrap();
    let file = dir.join("hello.txt");
    let mut w = fs.open_write(&file).unwrap();
    w.write_all(b"hello from manta").unwrap();
    w.flush().unwrap();
    drop(w);
    let mut back = String::new();
    fs.open_read(&file)
        .unwrap()
        .read_to_string(&mut back)
        .unwrap();
    assert_eq!(back, "hello from manta");
    fs.rename(&file, &dir.join("renamed.txt")).unwrap();
    assert!(fs.stat(&dir.join("renamed.txt")).is_ok());
    fs.remove_tree(&dir).unwrap();
}
