//! FTPS backend against a real server: READ-ONLY (connect + list).
//!
//! Ignored by default and never run in CI. Credentials come from the macOS
//! Keychain at run time (generic password, service `manta-update-ftp`,
//! account = FTP user, the same item `scripts/publish-update.sh` uses); the
//! password is never printed.
//!
//! The server comes only from the environment (nothing is hard-coded); without
//! `MANTA_FTPS_LIVE_HOST` every test here skips.
//!
//! ```text
//! MANTA_FTPS_LIVE_HOST=ftp.example.com cargo test -p manta-vfs --test ftps_live -- --ignored --nocapture
//! MANTA_FTPS_LIVE_HOST=ftp.example.com MANTA_FTP_DATA_TLS=require cargo test …
//! ```
//!
//! The only write: `remove_selftest_file` deletes `manta-upload-selftest.txt`
//! (the file a `manta-update-upload` self-test leaves behind) and nothing else.

use std::process::Command;

use manta_core::{Auth, ConnectionConfig, FtpDataChannel, FtpOptions, Protocol, VfsPath};

const SERVICE: &str = "manta-update-ftp";

/// (account, password) of the Keychain item, or `None` when there is none.
fn keychain_credentials() -> Option<(String, String)> {
    let service = std::env::var("MANTA_FTP_KEYCHAIN_SERVICE").unwrap_or_else(|_| SERVICE.into());
    // Attributes only (no -w): the account name.
    let out = Command::new("security")
        .args(["find-generic-password", "-s", &service])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let attrs = String::from_utf8_lossy(&out.stdout);
    let user = attrs.lines().find_map(|l| {
        l.trim()
            .strip_prefix("\"acct\"<blob>=\"")?
            .strip_suffix('"')
            .map(str::to_string)
    })?;
    let out = Command::new("security")
        .args(["find-generic-password", "-s", &service, "-a", &user, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let pw = String::from_utf8(out.stdout)
        .ok()?
        .trim_end_matches('\n')
        .to_string();
    Some((user, pw))
}

/// The one file the tests may delete.
const SELFTEST_FILE: &str = "manta-upload-selftest.txt";

fn live_config() -> Option<ConnectionConfig> {
    let Ok(host) = std::env::var("MANTA_FTPS_LIVE_HOST") else {
        eprintln!("skipped: set MANTA_FTPS_LIVE_HOST to the FTPS server (e.g. ftp.example.com)");
        return None;
    };
    let Some((user, password)) = keychain_credentials() else {
        eprintln!("skipped: no Keychain item {SERVICE}");
        return None;
    };
    let data_channel = match std::env::var("MANTA_FTP_DATA_TLS").as_deref() {
        Ok("require") => FtpDataChannel::Require,
        Ok("off") => FtpDataChannel::Off,
        _ => FtpDataChannel::Auto,
    };
    let cfg = ConnectionConfig {
        name: "live".into(),
        protocol: Protocol::Ftps,
        host: host.clone(),
        port: 21,
        username: user,
        auth: Auth::Password(password),
        initial_path: None,
        s3: None,
        container: None,
        ftp: Some(Box::new(FtpOptions { data_channel })),
    };
    eprintln!("data channel policy {data_channel:?}");
    Some(cfg)
}

#[test]
#[ignore = "talks to the real release server; run by hand"]
fn list_root_read_only() {
    let Some(cfg) = live_config() else { return };
    let fs = manta_vfs::connect(&cfg).expect("connect + TLS login");
    eprintln!("connected: {}", fs.label());
    let list = fs.list(&VfsPath::root());
    eprintln!("notice: {:?}", fs.notice());
    let entries = list.expect("list /");
    for e in &entries {
        eprintln!(
            "  {:<6} {:>10}  {}",
            format!("{:?}", e.kind),
            e.size,
            e.name
        );
    }
    assert!(!entries.is_empty() || std::env::var("MANTA_FTPS_LIVE_ALLOW_EMPTY").is_ok());
}

#[test]
#[ignore = "deletes manta-upload-selftest.txt on the real release server; run by hand"]
fn remove_selftest_file() {
    let Some(cfg) = live_config() else { return };
    let fs = manta_vfs::connect(&cfg).expect("connect + TLS login");
    let path = VfsPath::root().join(SELFTEST_FILE);
    match fs.remove_file(&path) {
        Ok(()) => eprintln!("deleted {path}"),
        Err(manta_core::VfsError::NotFound(_)) => eprintln!("{path} was not there"),
        Err(e) => panic!("deleting {path}: {e}"),
    }
    let names: Vec<String> = fs
        .list(&VfsPath::root())
        .expect("list /")
        .into_iter()
        .map(|e| e.name)
        .collect();
    eprintln!("now on the server: {names:?}");
    assert!(!names.iter().any(|n| n == SELFTEST_FILE));
}

/// Find `name` in `/` or one folder below.
fn find_file(fs: &dyn manta_core::Vfs, name: &str) -> Option<manta_core::Entry> {
    let root = fs.list(&VfsPath::root()).ok()?;
    if let Some(e) = root.iter().find(|e| e.name == name && !e.is_dir()) {
        return Some(e.clone());
    }
    root.iter().filter(|e| e.is_dir()).find_map(|d| {
        fs.list(&d.path)
            .ok()?
            .into_iter()
            .find(|e| e.name == name && !e.is_dir())
    })
}

/// READ-ONLY: download the public release zip with a connection drop in the
/// middle (the reader is dropped, i.e. `ABOR`), then let the transfer
/// manager continue it from the journal with `REST`, and compare the
/// SHA-256 with the HTTPS copy of the same file.
#[test]
#[ignore = "downloads a public release file from the real server; run by hand"]
fn resume_release_download_read_only() {
    use std::io::Read;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use manta_ops::journal::{Journal, JournalJob, JournalState};
    use manta_ops::resume::{part_path, Partial, Progress};
    use manta_ops::{
        ConflictPolicy, Endpoint, JobKind, JobState, TransferManager, TransferOptions,
    };
    use sha2::{Digest, Sha256};

    const NAME: &str = "Manta-0.3.0.zip";
    const HTTPS: &str = "https://manta-filemanager.com/updates/Manta-0.3.0.zip";
    let Some(cfg) = live_config() else { return };
    let fs: manta_core::SharedVfs = manta_vfs::connect(&cfg).expect("connect + TLS login");
    let Some(src) = find_file(&*fs, NAME) else {
        eprintln!("skipped: {NAME} not found on the server");
        return;
    };
    eprintln!("found {} ({} bytes)", src.path, src.size);
    assert!(src.size > 1_000_000, "release zip is suspiciously small");

    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let dst = VfsPath::new(root.to_str().unwrap()).join(NAME);
    let part = part_path(&dst);

    // 1. Download about 40 %, then "lose the connection".
    let cut = src.size * 2 / 5;
    {
        let mut r = fs.open_read(&src.path).expect("RETR");
        let mut head = vec![0u8; cut as usize];
        r.read_exact(&mut head).expect("first part");
        std::fs::write(part.as_str(), &head).unwrap();
    }
    fs.reconnect().expect("reconnect");

    // 2. The journal left by the interruption; the manager continues with REST.
    let journal = root.join("transfers.json");
    let job = JournalJob {
        id: 1,
        kind: JobKind::Copy,
        title: format!("Copying \"{NAME}\""),
        state: JournalState::Waiting,
        conflict: ConflictPolicy::Overwrite,
        src: Endpoint::remote(&cfg),
        dst: Some(Endpoint::Local),
        sources: vec![src.path.to_string()],
        dst_dir: Some(VfsPath::new(root.to_str().unwrap()).to_string()),
        progress: Progress {
            partial: Some(Partial {
                src: src.path.to_string(),
                dst: dst.to_string(),
                part: Some(part.to_string()),
                size: src.size,
                mtime: src.modified,
                tag: None,
                session: None,
                choice: None,
            }),
            ..Default::default()
        },
        bytes_done: cut,
        bytes_total: src.size,
        files_done: 0,
        files_total: 1,
        errors: Vec::new(),
        log: Vec::new(),
    };
    Journal::new(journal.clone()).save(&[job]).unwrap();
    assert!(
        !std::fs::read_to_string(&journal)
            .unwrap()
            .contains("\"Password\""),
        "no password in the journal"
    );
    let local: manta_core::SharedVfs = Arc::new(manta_vfs::LocalFs::new());
    let tm = TransferManager::with_options(TransferOptions {
        max_parallel: 1,
        journal: Some(journal),
        ..Default::default()
    });
    let ids = tm.restore_interrupted(&|ep: &Endpoint| match ep {
        Endpoint::Local => Some(local.clone()),
        _ => Some(fs.clone()),
    });
    let start = Instant::now();
    let mut resumed = None;
    let st = loop {
        let st = tm.snapshot().into_iter().find(|s| s.id == ids[0]).unwrap();
        resumed = resumed.or(st.resume.resumed_from);
        if st.state.is_finished() {
            break st;
        }
        assert!(
            start.elapsed() < Duration::from_secs(600),
            "download timeout"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(resumed, Some(cut), "continued at the cut");
    let got = std::fs::read(dst.as_str()).unwrap();
    assert_eq!(got.len() as u64, src.size);

    // 3. Compare with the HTTPS copy.
    let https = root.join("https.zip");
    let ok = Command::new("curl")
        .args(["-sSfL", "-o"])
        .arg(&https)
        .arg(HTTPS)
        .status()
        .map(|s| s.success());
    if !matches!(ok, Ok(true)) {
        eprintln!("HTTPS copy unavailable; compared size only");
        return;
    }
    let a = Sha256::digest(&got);
    let b = Sha256::digest(std::fs::read(&https).unwrap());
    let hex = |d: &[u8]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
    eprintln!("sha256 resumed FTP download: {}", hex(&a));
    eprintln!("sha256 HTTPS copy:           {}", hex(&b));
    assert_eq!(a, b, "resumed download differs from the HTTPS copy");
}
