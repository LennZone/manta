//! The WebDAV backend against a real server: Apache `mod_dav` as shipped
//! with macOS (`/usr/sbin/httpd`), started on a free local port with a
//! throwaway config, document root and password files, and stopped
//! afterwards. Digest and Basic authentication, a root prefix, uploads
//! (buffered and streamed), ranged reads, MOVE, COPY, DELETE.
//!
//! Skips cleanly when httpd or its DAV modules are missing.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::{
    Auth, ConnectionConfig, Protocol, SharedVfs, Vfs, VfsError, VfsPath, WebDavOptions,
};
use manta_ops::{ConflictPolicy, Job, JobKind, JobState, TransferManager};
use manta_vfs::WebDavFs;

const MODULES: &str = "/usr/libexec/apache2";
const USER: &str = "anna";
const PASS: &str = "s3cret pass";

struct Apache {
    child: Child,
    port: u16,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

impl Drop for Apache {
    fn drop(&mut self) {
        // SIGTERM: the parent stops its prefork children too (SIGKILL
        // would leave them running).
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap()
}

fn md5_hex(s: &str) -> Option<String> {
    let out = Command::new("/sbin/md5")
        .args(["-q", "-s", s])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn start() -> Option<Apache> {
    let httpd = Path::new("/usr/sbin/httpd");
    let needed = [
        "mod_dav.so",
        "mod_dav_fs.so",
        "mod_auth_digest.so",
        "mod_auth_basic.so",
        "mod_authn_file.so",
        "mod_authz_user.so",
        "mod_mpm_prefork.so",
    ];
    if !httpd.exists() || needed.iter().any(|m| !Path::new(MODULES).join(m).exists()) {
        eprintln!("skipping: macOS Apache with mod_dav not found");
        return None;
    }
    let htpasswd = Path::new("/usr/sbin/htpasswd");
    if !htpasswd.exists() {
        eprintln!("skipping: htpasswd not found");
        return None;
    }
    let dir = tempfile::tempdir().ok()?;
    let d = dir.path();
    let root = d.join("www");
    for sub in ["digest/files/anna", "basic"] {
        std::fs::create_dir_all(root.join(sub)).ok()?;
    }
    std::fs::create_dir_all(d.join("lock")).ok()?;
    let ha1 = md5_hex(&format!("{USER}:manta:{PASS}"))?;
    std::fs::write(d.join("digest.pw"), format!("{USER}:manta:{ha1}\n")).ok()?;
    let st = Command::new(htpasswd)
        .args(["-cbB"])
        .arg(d.join("basic.pw"))
        .args([USER, PASS])
        .stderr(Stdio::null())
        .status()
        .ok()?;
    if !st.success() {
        eprintln!("skipping: htpasswd failed");
        return None;
    }
    let port = free_port();
    let m = |name: &str, file: &str| format!("LoadModule {name}_module {MODULES}/{file}\n");
    let mut conf = String::new();
    for (n, f) in [
        ("mpm_prefork", "mod_mpm_prefork.so"),
        ("unixd", "mod_unixd.so"),
        ("authn_core", "mod_authn_core.so"),
        ("authz_core", "mod_authz_core.so"),
        ("authn_file", "mod_authn_file.so"),
        ("authz_user", "mod_authz_user.so"),
        ("auth_basic", "mod_auth_basic.so"),
        ("auth_digest", "mod_auth_digest.so"),
        ("dav", "mod_dav.so"),
        ("dav_fs", "mod_dav_fs.so"),
    ] {
        conf.push_str(&m(n, f));
    }
    conf.push_str(&format!(
        r#"ServerName 127.0.0.1
Listen 127.0.0.1:{port}
PidFile "{d}/httpd.pid"
DefaultRuntimeDir "{d}"
Mutex file:{d}/lock default
ErrorLog "{d}/error.log"
LogLevel warn
DocumentRoot "{www}"
DavLockDB "{d}/lock/DavLock"
StartServers 2
MaxRequestWorkers 8
<Directory "{www}">
    Require all granted
</Directory>
<Location "/digest">
    Dav On
    AuthType Digest
    AuthName "manta"
    AuthDigestProvider file
    AuthUserFile "{d}/digest.pw"
    Require valid-user
</Location>
<Location "/basic">
    Dav On
    AuthType Basic
    AuthName "manta"
    AuthBasicProvider file
    AuthUserFile "{d}/basic.pw"
    Require valid-user
</Location>
"#,
        d = d.display(),
        www = root.display()
    ));
    let conf_path = d.join("httpd.conf");
    std::fs::write(&conf_path, conf).ok()?;
    // Its own process group: on SIGTERM httpd signals its whole group,
    // which must not include the test runner.
    use std::os::unix::process::CommandExt as _;
    let child = Command::new(httpd)
        .process_group(0)
        .arg("-f")
        .arg(&conf_path)
        .arg("-DFOREGROUND")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let mut apache = Apache {
        child,
        port,
        root,
        _dir: dir,
    };
    let start = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        if let Ok(Some(status)) = apache.child.try_wait() {
            let mut err = String::new();
            if let Some(mut e) = apache.child.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            let log =
                std::fs::read_to_string(apache._dir.path().join("error.log")).unwrap_or_default();
            eprintln!("skipping: httpd exited ({status}): {err} {log}");
            return None;
        }
        if start.elapsed() > Duration::from_secs(10) {
            eprintln!("skipping: httpd did not start");
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Some(apache)
}

fn config(port: u16, root: &str, password: &str) -> ConnectionConfig {
    ConnectionConfig {
        name: "apache".into(),
        protocol: Protocol::WebDav,
        host: "127.0.0.1".into(),
        port,
        username: USER.into(),
        auth: Auth::Password(password.into()),
        initial_path: None,
        s3: None,
        container: None,
        ftp: None,
        webdav: Some(Box::new(WebDavOptions {
            root: root.into(),
            preset: "generic".into(),
        })),
        smb: None,
    }
}

fn body(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i % 251) as u8 ^ (i / 251) as u8)
        .collect()
}

fn read(fs: &dyn Vfs, p: &str) -> Vec<u8> {
    let mut out = Vec::new();
    fs.open_read(&VfsPath::new(p))
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    out
}

fn exercise(fs: &WebDavFs, disk: &Path) {
    // Write small (buffered) and large (streamed) files.
    let mut w = fs.open_write(&VfsPath::new("/Hello Wörld.txt")).unwrap();
    w.write_all(b"hello").unwrap();
    w.flush().unwrap();
    drop(w);
    assert_eq!(
        std::fs::read(disk.join("Hello Wörld.txt")).unwrap(),
        b"hello"
    );
    let big = body(manta_vfs::webdav::BUFFERED_MAX + 3_000_000);
    let mut w = fs.open_write(&VfsPath::new("/big.bin")).unwrap();
    for c in big.chunks(1 << 20) {
        w.write_all(c).unwrap();
    }
    w.flush().unwrap();
    drop(w);
    assert_eq!(std::fs::read(disk.join("big.bin")).unwrap(), big);

    // Folders, listing, stat.
    fs.create_dir(&VfsPath::new("/A & B")).unwrap();
    assert!(matches!(
        fs.create_dir(&VfsPath::new("/A & B")),
        Err(VfsError::AlreadyExists(_))
    ));
    let mut names: Vec<(String, bool)> = fs
        .list(&VfsPath::root())
        .unwrap()
        .into_iter()
        .map(|e| (e.name.clone(), e.is_dir()))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            ("A & B".to_string(), true),
            ("Hello Wörld.txt".to_string(), false),
            ("big.bin".to_string(), false)
        ]
    );
    let st = fs.stat(&VfsPath::new("/big.bin")).unwrap();
    assert_eq!(st.size, big.len() as u64);
    assert!(st.modified.is_some());
    assert!(fs.content_tag(&VfsPath::new("/big.bin")).is_some());

    // Ranged read.
    let mut tail = Vec::new();
    fs.open_read_at(&VfsPath::new("/big.bin"), 10_000_000)
        .unwrap()
        .read_to_end(&mut tail)
        .unwrap();
    assert_eq!(tail, big[10_000_000..]);

    // MOVE a file and a folder, COPY a folder, DELETE.
    fs.rename(
        &VfsPath::new("/Hello Wörld.txt"),
        &VfsPath::new("/A & B/moved.txt"),
    )
    .unwrap();
    assert_eq!(read(fs, "/A & B/moved.txt"), b"hello");
    fs.rename(&VfsPath::new("/A & B"), &VfsPath::new("/Renamed"))
        .unwrap();
    fs.server_copy(&VfsPath::new("/Renamed"), &VfsPath::new("/Copy"))
        .unwrap();
    assert_eq!(read(fs, "/Copy/moved.txt"), b"hello");
    assert!(matches!(
        fs.remove_dir(&VfsPath::new("/Copy")),
        Err(VfsError::DirectoryNotEmpty(_))
    ));
    fs.remove_file(&VfsPath::new("/Copy/moved.txt")).unwrap();
    fs.remove_dir(&VfsPath::new("/Copy")).unwrap();
    fs.remove_tree(&VfsPath::new("/Renamed")).unwrap();
    fs.remove_file(&VfsPath::new("/big.bin")).unwrap();
    assert!(fs.list(&VfsPath::root()).unwrap().is_empty());
    assert!(!disk.join("Renamed").exists());
}

#[test]
fn apache_mod_dav_digest_and_basic() {
    let Some(apache) = start() else {
        return;
    };
    // Digest, below a Nextcloud-like prefix.
    let root = "/digest/files/anna";
    let fs = WebDavFs::connect(&config(apache.port, root, PASS)).expect("digest connect");
    exercise(&fs, &apache.root.join("digest/files/anna"));
    let err = WebDavFs::connect(&config(apache.port, root, "wrong"))
        .err()
        .expect("wrong password");
    assert!(matches!(err, VfsError::Auth(_)), "{err:?}");
    let err = WebDavFs::connect(&config(apache.port, "/digest/nope", PASS))
        .err()
        .expect("missing root");
    assert!(matches!(err, VfsError::NotFound(_)), "{err:?}");

    // Basic.
    let fs = WebDavFs::connect(&config(apache.port, "/basic", PASS)).expect("basic connect");
    exercise(&fs, &apache.root.join("basic"));

    // Through the transfer engine: local folder up and down again.
    let fs: SharedVfs = Arc::new(fs);
    let td = tempfile::tempdir().unwrap();
    std::fs::create_dir(td.path().join("up")).unwrap();
    std::fs::write(td.path().join("up/a.bin"), body(500_000)).unwrap();
    let local: SharedVfs = Arc::new(manta_vfs::LocalFs::new());
    let tm = TransferManager::new(2);
    let wait = |id| {
        let start = Instant::now();
        loop {
            let st = tm.snapshot().into_iter().find(|s| s.id == id).unwrap();
            if st.state.is_finished() {
                return st;
            }
            assert!(start.elapsed() < Duration::from_secs(60), "{st:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let id = tm.enqueue(Job {
        kind: JobKind::Copy,
        src_fs: local.clone(),
        sources: vec![VfsPath::new(&td.path().join("up").to_string_lossy())],
        dst_fs: Some(fs.clone()),
        dst_dir: Some(VfsPath::root()),
        conflict: ConflictPolicy::Overwrite,
    });
    assert_eq!(wait(id).state, JobState::Done);
    assert_eq!(
        std::fs::read(apache.root.join("basic/up/a.bin")).unwrap(),
        body(500_000)
    );
    let back = td.path().join("back");
    std::fs::create_dir(&back).unwrap();
    let id = tm.enqueue(Job {
        kind: JobKind::Copy,
        src_fs: fs,
        sources: vec![VfsPath::new("/up")],
        dst_fs: Some(local),
        dst_dir: Some(VfsPath::new(&back.to_string_lossy())),
        conflict: ConflictPolicy::Overwrite,
    });
    assert_eq!(wait(id).state, JobState::Done);
    assert_eq!(std::fs::read(back.join("up/a.bin")).unwrap(), body(500_000));
}
