//! SFTP resume support against a throwaway, unprivileged `sshd` on
//! 127.0.0.1 (random port, temporary host and client keys, key auth only,
//! internal-sftp). Skipped when `sshd` / `ssh-keygen` aren't installed or
//! the server doesn't come up.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::{Auth, ConnectionConfig, Protocol, SharedVfs, VfsError, VfsPath};
use manta_ops::journal::{Journal, JournalJob, JournalState};
use manta_ops::resume::{part_path, Partial, Progress};
use manta_ops::{ConflictPolicy, Endpoint, JobKind, JobState, TransferManager, TransferOptions};
use manta_vfs::{LocalFs, SftpFs};

fn find(names: &[&str]) -> Option<PathBuf> {
    names.iter().map(PathBuf::from).find(|p| p.exists())
}

/// The running test server; killed on drop.
struct Sshd {
    child: Child,
}

impl Drop for Sshd {
    /// Kills the listener and every session process it forked (its own
    /// process group), like a server that drops off the network.
    fn drop(&mut self) {
        // Session processes may start their own session: find them by parent.
        fn descendants(pid: u32, out: &mut Vec<u32>) {
            let Ok(o) = Command::new("pgrep")
                .args(["-P", &pid.to_string()])
                .output()
            else {
                return;
            };
            for child in String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .filter_map(|p| p.parse().ok())
            {
                out.push(child);
                descendants(child, out);
            }
        }
        let mut pids = Vec::new();
        descendants(self.child.id(), &mut pids);
        let _ = self.child.kill();
        for pid in pids {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{}", self.child.id())])
            .status();
        let _ = self.child.wait();
    }
}

struct Setup {
    dir: tempfile::TempDir,
    sshd: PathBuf,
    port: u16,
}

impl Setup {
    fn new() -> Option<Setup> {
        let sshd = find(&[
            "/opt/homebrew/sbin/sshd",
            "/usr/local/sbin/sshd",
            "/usr/sbin/sshd",
        ])?;
        let keygen = find(&["/opt/homebrew/bin/ssh-keygen", "/usr/bin/ssh-keygen"])?;
        let dir = tempfile::tempdir().ok()?;
        let d = dir.path();
        for key in ["host_key", "client_key"] {
            let ok = Command::new(&keygen)
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(d.join(key))
                .status()
                .ok()?
                .success();
            if !ok {
                return None;
            }
        }
        std::fs::copy(d.join("client_key.pub"), d.join("authorized_keys")).ok()?;
        let port = TcpListener::bind("127.0.0.1:0")
            .ok()?
            .local_addr()
            .ok()?
            .port();
        let cfg = format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {d}/host_key\nAuthorizedKeysFile {d}/authorized_keys\n\
             PasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\n\
             PidFile {d}/sshd.pid\nSubsystem sftp internal-sftp\n",
            d = d.display()
        );
        std::fs::write(d.join("sshd_config"), cfg).ok()?;
        Some(Setup { dir, sshd, port })
    }

    fn start(&self) -> Option<Sshd> {
        use std::os::unix::process::CommandExt;
        let child = Command::new(&self.sshd)
            .process_group(0)
            .args(["-D", "-f"])
            .arg(self.dir.path().join("sshd_config"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let s = Sshd { child };
        let start = Instant::now();
        while std::net::TcpStream::connect(("127.0.0.1", self.port)).is_err() {
            if start.elapsed() > Duration::from_secs(5) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(s)
    }

    fn config(&self) -> ConnectionConfig {
        ConnectionConfig {
            name: String::new(),
            protocol: Protocol::Sftp,
            host: "127.0.0.1".into(),
            port: self.port,
            username: std::env::var("USER").unwrap_or_default(),
            auth: Auth::KeyFile {
                path: self.dir.path().join("client_key").display().to_string(),
                passphrase: None,
            },
            initial_path: None,
            s3: None,
            container: None,
            ftp: None,
        }
    }
}

/// All tests of this binary check host keys against one temporary
/// `known_hosts` file (never the user's own).
fn test_known_hosts() -> &'static Path {
    static FILE: std::sync::OnceLock<(tempfile::TempDir, PathBuf)> = std::sync::OnceLock::new();
    let (_, file) = FILE.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("known_hosts");
        manta_vfs::known_hosts::set_known_hosts_file(Some(file.clone()));
        (dir, file)
    });
    file
}

impl Setup {
    /// The server's public host key blob (from `host_key.pub`).
    fn host_key(&self) -> Vec<u8> {
        use base64::Engine as _;
        let text = std::fs::read_to_string(self.dir.path().join("host_key.pub")).unwrap();
        let b64 = text.split_whitespace().nth(1).unwrap();
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap()
    }

    /// First contact: the unknown key is reported, accepted, and the next
    /// connect succeeds. `None` if the test server is unusable.
    fn connect_trusting(&self) -> Option<SftpFs> {
        test_known_hosts();
        match SftpFs::connect(&self.config()) {
            Ok(fs) => Some(fs),
            Err(VfsError::UnknownHostKey(info)) => {
                assert_eq!(info.key, self.host_key(), "the server's key is reported");
                assert_eq!(info.key_type, "ssh-ed25519");
                assert_eq!(
                    info.fingerprint,
                    manta_vfs::known_hosts::fingerprint(&self.host_key())
                );
                manta_vfs::known_hosts::trust_host_key(&info).expect("append to known_hosts");
                match SftpFs::connect(&self.config()) {
                    Ok(fs) => Some(fs),
                    Err(e) => {
                        assert!(
                            !matches!(
                                e,
                                VfsError::UnknownHostKey(_) | VfsError::HostKeyMismatch(_)
                            ),
                            "{e}"
                        );
                        eprintln!("skipping: can't log in to the test sshd: {e}");
                        None
                    }
                }
            }
            Err(e) => {
                eprintln!("skipping: can't log in to the test sshd: {e}");
                None
            }
        }
    }
}

fn vp(p: &Path) -> VfsPath {
    VfsPath::new(p.to_str().unwrap())
}

fn noise(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect()
}

#[test]
fn sftp_resume_against_a_local_sshd() {
    let Some(setup) = Setup::new() else {
        eprintln!("skipping: sshd or ssh-keygen not available");
        return;
    };
    let Some(server) = setup.start() else {
        eprintln!("skipping: the test sshd did not start");
        return;
    };
    let Some(fs) = setup.connect_trusting() else {
        return;
    };
    let fs: SharedVfs = Arc::new(fs);
    let caps = fs.supports_resume();
    assert!(caps.read_at && caps.append);

    let work = tempfile::tempdir().unwrap();
    let root = work.path().canonicalize().unwrap();
    let data = noise(3_000_000);
    std::fs::write(root.join("remote.bin"), &data).unwrap();

    // Seek on the remote file.
    let mut tail = Vec::new();
    fs.open_read_at(&vp(&root.join("remote.bin")), 2_000_000)
        .unwrap()
        .read_to_end(&mut tail)
        .unwrap();
    assert!(tail == data[2_000_000..]);
    assert!(fs
        .open_read_at(&vp(&root.join("remote.bin")), 3_000_001)
        .is_err());

    // Write at the end (no truncation).
    std::fs::write(root.join("append.txt"), b"head-").unwrap();
    let mut w = fs.open_append(&vp(&root.join("append.txt"))).unwrap();
    w.write_all(b"tail").unwrap();
    w.flush().unwrap();
    drop(w);
    assert_eq!(
        std::fs::read(root.join("append.txt")).unwrap(),
        b"head-tail"
    );
    let mut w = fs.open_append(&vp(&root.join("new.txt"))).unwrap();
    w.write_all(b"created").unwrap();
    w.flush().unwrap();
    drop(w);
    assert_eq!(std::fs::read(root.join("new.txt")).unwrap(), b"created");

    // A live session: reconnect is a no-op.
    fs.reconnect().unwrap();

    // The server goes away (all sessions die) and comes back: reconnect
    // logs in again and the backend works on.
    drop(server);
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        fs.stat(&vp(&root.join("remote.bin"))).is_err(),
        "the old session is dead"
    );
    assert!(fs.reconnect().is_err(), "nobody listens");
    let Some(_server) = setup.start() else {
        panic!("the test sshd did not restart");
    };
    fs.reconnect().unwrap();
    assert_eq!(
        fs.stat(&vp(&root.join("remote.bin"))).unwrap().size,
        data.len() as u64
    );

    // An interrupted download (1.5 MB in the part file) continues through
    // the transfer manager with a seek on the server.
    let down = root.join("down");
    std::fs::create_dir_all(&down).unwrap();
    std::fs::write(down.join(".remote.bin.manta-part"), &data[..1_500_000]).unwrap();
    let src = fs.stat(&vp(&root.join("remote.bin"))).unwrap();
    let dst = vp(&down).join("remote.bin");
    let journal = root.join("transfers.json");
    let job = JournalJob {
        id: 1,
        kind: JobKind::Copy,
        title: "download".into(),
        state: JournalState::Running,
        conflict: ConflictPolicy::Overwrite,
        src: Endpoint::remote(&setup.config()),
        dst: Some(Endpoint::Local),
        sources: vec![src.path.to_string()],
        dst_dir: Some(vp(&down).to_string()),
        progress: Progress {
            partial: Some(Partial {
                src: src.path.to_string(),
                dst: dst.to_string(),
                part: Some(part_path(&dst).to_string()),
                size: src.size,
                mtime: src.modified,
                tag: None,
                session: None,
                choice: None,
            }),
            ..Default::default()
        },
        bytes_done: 0,
        bytes_total: src.size,
        files_done: 0,
        files_total: 1,
        errors: Vec::new(),
        log: Vec::new(),
    };
    Journal::new(journal.clone()).save(&[job]).unwrap();
    let local: SharedVfs = Arc::new(LocalFs::new());
    let tm = TransferManager::with_options(TransferOptions {
        max_parallel: 1,
        journal: Some(journal),
        ..Default::default()
    });
    let ids = tm.restore_interrupted(&|ep: &Endpoint| match ep {
        Endpoint::Local => Some(local.clone()),
        _ => Some(fs.clone()),
    });
    let mut resumed = None;
    let start = Instant::now();
    let st = loop {
        let st = tm.snapshot().into_iter().find(|s| s.id == ids[0]).unwrap();
        resumed = resumed.or(st.resume.resumed_from);
        if st.state.is_finished() {
            break st;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "timeout");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(resumed, Some(1_500_000));
    assert!(std::fs::read(down.join("remote.bin")).unwrap() == data);
}

#[test]
fn sftp_host_key_tofu_and_mismatch() {
    let Some(setup) = Setup::new() else {
        eprintln!("skipping: sshd or ssh-keygen not available");
        return;
    };
    let Some(_server) = setup.start() else {
        eprintln!("skipping: the test sshd did not start");
        return;
    };
    let file = test_known_hosts();
    // Unknown -> accepted -> written as "[127.0.0.1]:port ssh-ed25519 ...".
    let Some(fs) = setup.connect_trusting() else {
        return;
    };
    drop(fs);
    let text = std::fs::read_to_string(file).unwrap();
    let pattern = format!("[127.0.0.1]:{} ssh-ed25519 ", setup.port);
    assert_eq!(
        text.lines().filter(|l| l.starts_with(&pattern)).count(),
        1,
        "{text}"
    );
    // Known now: connects without asking.
    SftpFs::connect(&setup.config()).expect("known key connects");

    // The server's key changes (reinstalled, or an attacker): refused.
    let keygen = find(&["/opt/homebrew/bin/ssh-keygen", "/usr/bin/ssh-keygen"]).unwrap();
    let new_key = setup.dir.path().join("host_key_new");
    assert!(Command::new(&keygen)
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&new_key)
        .status()
        .unwrap()
        .success());
    std::fs::copy(&new_key, setup.dir.path().join("host_key")).unwrap();
    std::fs::copy(
        new_key.with_extension("pub"),
        setup.dir.path().join("host_key.pub"),
    )
    .unwrap();
    drop(_server);
    let Some(_server) = setup.start() else {
        eprintln!("skipping: the test sshd did not restart");
        return;
    };
    match SftpFs::connect(&setup.config()) {
        Err(VfsError::HostKeyMismatch(info)) => {
            assert_eq!(info.key, setup.host_key());
            assert_eq!(info.port, setup.port);
            assert!(info.known_hosts.ends_with("known_hosts"));
            // Accepting a changed key is refused, the file stays as it was.
            assert!(matches!(
                manta_vfs::known_hosts::trust_host_key(&info),
                Err(VfsError::HostKeyMismatch(_))
            ));
            assert_eq!(std::fs::read_to_string(file).unwrap(), text);
        }
        Err(e) => panic!("expected a host key mismatch, got {e}"),
        Ok(_) => panic!("connected despite a changed host key"),
    }
}
