//! Resume support (`REST` + `RETR`, `APPE`, `reconnect`) against the fake
//! server, directly and through the transfer manager with a simulated
//! connection drop.

use std::io::{Read, Write};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::{
    Auth, ConnectionConfig, FtpDataChannel, FtpOptions, Protocol, SharedVfs, VfsPath,
};
use manta_ops::{
    Backoff, ConflictPolicy, Job, JobKind, JobState, TransferManager, TransferOptions,
};

use super::fake_server::{client_config, Options, Server};
use super::*;

fn config(server: &Server) -> ConnectionConfig {
    ConnectionConfig {
        name: "fake".into(),
        protocol: Protocol::Ftps,
        host: "127.0.0.1".into(),
        port: server.port,
        username: "manta".into(),
        auth: Auth::Password("secret".into()),
        initial_path: None,
        s3: None,
        container: None,
        ftp: Some(Box::new(FtpOptions {
            data_channel: FtpDataChannel::Require,
        })),
        webdav: None,
        smb: None,
    }
}

fn secure(server: &Server) -> SharedVfs {
    Arc::new(connect_secure_with(&config(server), client_config()).expect("connect"))
}

fn body(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i % 251) as u8 ^ (i / 251) as u8)
        .collect()
}

#[test]
fn rest_reads_from_an_offset_and_appe_appends() {
    let server = Server::start(Options::default());
    server
        .files
        .lock()
        .unwrap()
        .insert("a.bin".into(), body(10_000));
    let fs = secure(&server);
    let caps = fs.supports_resume();
    assert!(caps.read_at && caps.append && !caps.upload_session);

    let mut out = Vec::new();
    fs.open_read_at(&VfsPath::new("/a.bin"), 7_000)
        .expect("read_at")
        .read_to_end(&mut out)
        .expect("read");
    assert_eq!(out, body(10_000)[7_000..]);
    assert!(
        server.commands().iter().any(|c| c == "REST 7000"),
        "{:?}",
        server.commands()
    );

    let mut w = fs.open_append(&VfsPath::new("/a.bin")).expect("append");
    w.write_all(b"tail").expect("write");
    w.flush().expect("finish");
    drop(w);
    let stored = server.files.lock().unwrap().get("a.bin").cloned().unwrap();
    assert_eq!(stored.len(), 10_004);
    assert!(stored.ends_with(b"tail"));
    assert!(server.commands().iter().any(|c| c == "APPE /a.bin"));
    // Every encrypted data connection resumed the control connection's
    // TLS session (servers that require it), also for REST / APPE.
    assert!(server.seen.lock().unwrap().tls_data.iter().all(|r| *r));

    // reconnect drops the idle pool and logs in again.
    let before = server
        .commands()
        .iter()
        .filter(|c| c.starts_with("USER"))
        .count();
    fs.reconnect().expect("reconnect");
    let after = server
        .commands()
        .iter()
        .filter(|c| c.starts_with("USER"))
        .count();
    assert_eq!(after, before + 1);
}

fn wait(tm: &TransferManager, id: u64, f: impl Fn(&JobState) -> bool) -> manta_ops::JobStatus {
    let start = Instant::now();
    loop {
        let st = tm.snapshot().into_iter().find(|s| s.id == id).expect("job");
        if f(&st.state) {
            return st;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "timeout: {st:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn manager() -> TransferManager {
    TransferManager::with_options(TransferOptions {
        max_parallel: 1,
        backoff: Backoff {
            base: Duration::from_millis(50),
            max: Duration::from_millis(200),
        },
        ..Default::default()
    })
}

#[test]
fn download_resumes_after_a_drop_with_rest() {
    let server = Server::start(Options::default());
    let content = body(3_000_000);
    server
        .files
        .lock()
        .unwrap()
        .insert("big.iso".into(), content.clone());
    let fs = secure(&server);
    let td = tempfile::tempdir().unwrap();
    let local: SharedVfs = Arc::new(crate::LocalFs::new());
    let dst = VfsPath::new(&td.path().to_string_lossy());

    // The download is cut after 1 MB and the server stays unreachable for a
    // moment, so the first reconnect fails too.
    *server.faults.cut_download.lock().unwrap() = Some(1_000_000);
    server.faults.down.store(true, Ordering::SeqCst);
    let tm = manager();
    let id = tm.enqueue(Job {
        kind: JobKind::Copy,
        src_fs: fs.clone(),
        sources: vec![VfsPath::new("/big.iso")],
        dst_fs: Some(local.clone()),
        dst_dir: Some(dst.clone()),
        conflict: ConflictPolicy::Overwrite,
    });
    let st = wait(
        &tm,
        id,
        |s| matches!(s, JobState::WaitingForNetwork { attempt, .. } if *attempt >= 2),
    );
    assert!(!st.state.is_finished());
    let part = td.path().join(".big.iso.manta-part");
    assert_eq!(std::fs::metadata(&part).unwrap().len(), 1_000_000);
    server.faults.down.store(false, Ordering::SeqCst);
    let st = wait(&tm, id, JobState::is_finished);
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(std::fs::read(td.path().join("big.iso")).unwrap(), content);
    assert!(!part.exists());
    assert!(
        // 64 KiB before the partial's size: compared with the end of
        // the partial file, then the download goes on.
        server
            .commands()
            .iter()
            .any(|c| c == &format!("REST {}", 1_000_000 - 64 * 1024)),
        "{:?}",
        server.commands()
    );
}

#[test]
fn upload_resumes_after_a_drop_with_appe() {
    let server = Server::start(Options::default());
    let fs = secure(&server);
    let td = tempfile::tempdir().unwrap();
    let content = body(2_500_000);
    std::fs::write(td.path().join("up.bin"), &content).unwrap();
    let local: SharedVfs = Arc::new(crate::LocalFs::new());
    let src = VfsPath::new(&td.path().join("up.bin").to_string_lossy());

    *server.faults.cut_upload.lock().unwrap() = Some(700_000);
    let tm = manager();
    let id = tm.enqueue(Job {
        kind: JobKind::Copy,
        src_fs: local,
        sources: vec![src],
        dst_fs: Some(fs.clone()),
        dst_dir: Some(VfsPath::root()),
        conflict: ConflictPolicy::Overwrite,
    });
    let st = wait(&tm, id, JobState::is_finished);
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(
        server.files.lock().unwrap().get("up.bin").cloned().unwrap(),
        content
    );
    assert!(!server
        .files
        .lock()
        .unwrap()
        .contains_key(".up.bin.manta-part"));
    let cmds = server.commands();
    assert!(
        cmds.iter().any(|c| c == "STOR /.up.bin.manta-part"),
        "{cmds:?}"
    );
    assert!(
        cmds.iter().any(|c| c == "APPE /.up.bin.manta-part"),
        "{cmds:?}"
    );
    assert!(cmds.iter().any(|c| c == "RNTO /up.bin"), "{cmds:?}");
}
