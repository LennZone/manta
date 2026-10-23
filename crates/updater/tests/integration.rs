//! End-to-end tests of the engine against a local HTTP server on 127.0.0.1:
//! a fake, ad-hoc-signed app bundle is published, checked, downloaded,
//! prepared and installed into a temp "Applications" folder.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use ed25519_dalek::SigningKey;
use manta_updater::download::sha256_file;
use manta_updater::*;

// ---------------------------------------------------------------- tiny HTTP server

#[derive(Default)]
struct ServerState {
    files: HashMap<String, Vec<u8>>,
    /// Delay per 16 KiB chunk (to cancel mid-download).
    throttle: Option<Duration>,
    /// Request lines with their Range header, for assertions.
    log: Vec<String>,
    /// Path prefix redirects (301), like Apache's `Redirect 301 /old/ https://new/`.
    redirects: Vec<(String, String)>,
}

struct Server {
    port: u16,
    state: Arc<Mutex<ServerState>>,
}

impl Server {
    fn start() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(ServerState::default()));
        let st = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let st = st.clone();
                std::thread::spawn(move || {
                    let _ = handle(stream, &st);
                });
            }
        });
        Server { port, state }
    }
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}/{path}", self.port)
    }
    fn put(&self, path: &str, data: Vec<u8>) {
        self.state
            .lock()
            .unwrap()
            .files
            .insert(path.to_string(), data);
    }
    fn remove(&self, path: &str) {
        self.state.lock().unwrap().files.remove(path);
    }
    /// Answer every request below `from` (a path prefix such as `manta/`)
    /// with a 301 to `to` + the rest of the path.
    fn redirect(&self, from: &str, to: &str) {
        self.state
            .lock()
            .unwrap()
            .redirects
            .push((from.to_string(), to.to_string()));
    }
}

fn handle(stream: TcpStream, st: &Mutex<ServerState>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let path = line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .trim_start_matches('/')
        .split('?')
        .next()
        .unwrap()
        .to_string();
    let mut range: Option<u64> = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("range: bytes=") {
            range = v.trim().trim_end_matches('-').parse().ok();
        }
    }
    let (data, throttle, location) = {
        let mut s = st.lock().unwrap();
        s.log.push(format!("{path} range={range:?}"));
        let location = s.redirects.iter().find_map(|(from, to)| {
            path.strip_prefix(from.as_str())
                .map(|rest| format!("{to}{rest}"))
        });
        (s.files.get(&path).cloned(), s.throttle, location)
    };
    let mut out = stream;
    if let Some(location) = location {
        return write!(out, "HTTP/1.1 301 Moved Permanently\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    }
    let Some(data) = data else {
        return out.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    };
    let (status, body) = match range {
        Some(start) if (start as usize) < data.len() => (
            format!(
                "206 Partial Content\r\nContent-Range: bytes {start}-{}/{}",
                data.len() - 1,
                data.len()
            ),
            &data[start as usize..],
        ),
        _ => ("200 OK".to_string(), &data[..]),
    };
    write!(
        out,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    for chunk in body.chunks(16 * 1024) {
        if let Some(d) = throttle {
            std::thread::sleep(d);
        }
        out.write_all(chunk)?;
    }
    out.flush()
}

// ---------------------------------------------------------------- fixtures

const APP_ID: &str = "com.manta-filemanager.manta";

fn key() -> SigningKey {
    SigningKey::from_bytes(&[42u8; 32])
}

/// A minimal app bundle with a shell-script executable, ad-hoc signed.
fn make_app(dir: &Path, bundle_id: &str, version: &str) -> PathBuf {
    let app = dir.join("Manta.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_id}</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleVersion</key><string>{version}</string>
<key>CFBundleExecutable</key><string>manta</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#
        ),
    )
    .unwrap();
    let exe = app.join("Contents/MacOS/manta");
    std::fs::write(&exe, format!("#!/bin/sh\necho {version}\n")).unwrap();
    Command::new("chmod").arg("+x").arg(&exe).status().unwrap();
    let ok = Command::new("codesign")
        .args(["--force", "-s", "-"])
        .arg(&app)
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "codesign: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    app
}

fn zip_app(app: &Path, zip: &Path) {
    let ok = Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(app)
        .arg(zip)
        .status()
        .unwrap();
    assert!(ok.success());
}

/// A signed release entry for `zip_bytes`.
fn release_for(version: &str, zip_bytes: &[u8], url: &str) -> Release {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("z");
    std::fs::write(&p, zip_bytes).unwrap();
    let (sha256, size) = sha256_file(&p).unwrap();
    let mut r = Release {
        version: Version::parse(version).unwrap(),
        channel: None,
        published: Utc::now().fixed_offset(),
        min_macos: "11.0".into(),
        notes: Notes {
            en: "New things".into(),
            de: Some("Neues".into()),
        },
        url: url.into(),
        size,
        sha256,
        signature: String::new(),
    };
    keys::sign_release(&key(), APP_ID, &mut r).unwrap();
    r
}

fn publish(server: &Server, releases: Vec<Release>) -> String {
    let m = Manifest {
        schema: SCHEMA,
        app_id: APP_ID.into(),
        channel: Channel::Stable,
        generated: None,
        releases,
    };
    let json = m.to_json();
    server.put("feed/update.json", json.clone().into_bytes());
    server.put(
        "feed/update.json.sig",
        keys::sign_manifest(&key(), json.as_bytes()).into_bytes(),
    );
    server.url("feed/update.json")
}

fn opts() -> CheckOptions {
    CheckOptions {
        key: key().verifying_key(),
        app_id: APP_ID.into(),
        os_version: None,
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    server: Server,
    installed: PathBuf,
    zip_bytes: Vec<u8>,
    feed: String,
    downloads: PathBuf,
}

/// Installed 0.1.0 in <tmp>/Applications, 0.2.0 published as a ZIP.
fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let apps = tmp.path().join("Applications");
    std::fs::create_dir_all(&apps).unwrap();
    let installed = make_app(&apps, APP_ID, "0.1.0");
    let build = tmp.path().join("build");
    std::fs::create_dir_all(&build).unwrap();
    let new_app = make_app(&build, APP_ID, "0.2.0");
    let zip = tmp.path().join("Manta-0.2.0.zip");
    zip_app(&new_app, &zip);
    let zip_bytes = std::fs::read(&zip).unwrap();
    let server = Server::start();
    server.put("feed/Manta-0.2.0.zip", zip_bytes.clone());
    let feed = publish(
        &server,
        vec![release_for("0.2.0", &zip_bytes, "Manta-0.2.0.zip")],
    );
    let downloads = tmp.path().join("downloads");
    Fixture {
        server,
        installed,
        zip_bytes,
        feed,
        downloads,
        _tmp: tmp,
    }
}

fn version_of(app: &Path) -> String {
    bundle::read_bundle_info(app).unwrap().version
}

// ---------------------------------------------------------------- tests

/// Installs that poll the old feed URL keep working after the old folder is
/// redirected (`Redirect 301 /manta/ https://manta-filemanager.com/updates/`):
/// update.json, update.json.sig (derived from the *requested* URL) and the
/// ZIP (relative URL resolved against the requested URL, or absolute) all
/// follow the redirect.
#[test]
fn old_feed_url_follows_a_folder_redirect() {
    let f = fixture();
    let server = &f.server;
    // Move the feed to updates/ and redirect the old folder there.
    for name in ["update.json", "update.json.sig", "Manta-0.2.0.zip"] {
        let data = server
            .state
            .lock()
            .unwrap()
            .files
            .remove(&format!("feed/{name}"))
            .unwrap();
        server.put(&format!("updates/{name}"), data);
    }
    server.redirect("feed/", &server.url("updates/"));
    let v010 = Version::parse("0.1.0").unwrap();

    let release = check_with(&opts(), &f.feed, &v010, Channel::Stable, None)
        .unwrap()
        .expect("update via redirect");
    assert_eq!(
        release.url,
        server.url("feed/Manta-0.2.0.zip"),
        "resolved against the requested URL"
    );
    let log = server.state.lock().unwrap().log.clone();
    for p in [
        "feed/update.json",
        "updates/update.json",
        "feed/update.json.sig",
        "updates/update.json.sig",
    ] {
        assert!(
            log.iter().any(|l| l.starts_with(&format!("{p} "))),
            "{p} not requested: {log:?}"
        );
    }
    let cancel = AtomicBool::new(false);
    let dl = download_with(
        &opts().key,
        APP_ID,
        &release,
        &f.downloads,
        &mut |_, _| {},
        &cancel,
    )
    .unwrap();
    assert_eq!(std::fs::read(dl.path()).unwrap(), f.zip_bytes);

    // An absolute ZIP URL on the new location works for the redirected feed too.
    let abs = server.url("updates/Manta-0.2.0.zip");
    let json = Manifest {
        schema: SCHEMA,
        app_id: APP_ID.into(),
        channel: Channel::Stable,
        generated: None,
        releases: vec![release_for("0.2.0", &f.zip_bytes, &abs)],
    }
    .to_json();
    server.put(
        "updates/update.json.sig",
        keys::sign_manifest(&key(), json.as_bytes()).into_bytes(),
    );
    server.put("updates/update.json", json.into_bytes());
    let release = check_with(&opts(), &f.feed, &v010, Channel::Stable, None)
        .unwrap()
        .expect("update");
    assert_eq!(release.url, abs);
}

#[test]
fn full_update_check_download_prepare_install() {
    let f = fixture();
    let v010 = Version::parse("0.1.0").unwrap();

    let release = check_with(&opts(), &f.feed, &v010, Channel::Stable, None)
        .unwrap()
        .expect("update available");
    assert_eq!(release.version.to_string(), "0.2.0");
    assert_eq!(
        release.url,
        f.server.url("feed/Manta-0.2.0.zip"),
        "relative URL resolved"
    );
    assert!(
        check_with(&opts(), &f.feed, &release.version, Channel::Stable, None)
            .unwrap()
            .is_none()
    );

    let mut seen = Vec::new();
    let cancel = AtomicBool::new(false);
    let dl = download_with(
        &opts().key,
        APP_ID,
        &release,
        &f.downloads,
        &mut |d, t| seen.push((d, t)),
        &cancel,
    )
    .unwrap();
    assert_eq!(
        seen.last(),
        Some(&(f.zip_bytes.len() as u64, f.zip_bytes.len() as u64))
    );
    assert!(
        seen.windows(2).all(|w| w[0].0 <= w[1].0),
        "progress is monotonic"
    );
    assert_eq!(std::fs::read(dl.path()).unwrap(), f.zip_bytes);
    assert!(!f.downloads.join("Manta-0.2.0.zip.part").exists());

    let prepared = prepare_with(
        &dl,
        &PrepareOptions {
            expected_bundle_id: APP_ID.into(),
            current_bundle: Some(f.installed.clone()),
            work_dir: None,
        },
    )
    .unwrap();
    assert_eq!(prepared.signing, bundle::Signing::AdHoc);

    let mut plan = install(prepared, &f.installed).unwrap();
    assert!(!plan.needs_admin);
    plan.backup = BackupDisposal::Keep;
    let outcome = plan.apply().unwrap();
    assert_eq!(outcome.installed, f.installed);
    assert_eq!(version_of(&f.installed), "0.2.0");
    let backup = outcome.backup.expect("backup kept");
    assert_eq!(version_of(&backup), "0.1.0");
    assert_eq!(
        backup.parent(),
        f.installed.parent(),
        "backup on the same volume, next to the app"
    );
    let out = Command::new(f.installed.join("Contents/MacOS/manta"))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0.2.0");
    Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&f.installed)
        .status()
        .unwrap()
        .success()
        .then_some(())
        .unwrap();

    // Only the app and the hidden backup are in "Applications" (no stage leftovers).
    let names: Vec<String> = std::fs::read_dir(f.installed.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");

    // Downloading again reuses the verified file.
    let again = download_with(
        &opts().key,
        APP_ID,
        &release,
        &f.downloads,
        &mut |_, _| {},
        &cancel,
    )
    .unwrap();
    assert_eq!(again.path(), dl.path());
}

#[test]
fn delete_backup_disposal() {
    let f = fixture();
    let release = check_with(
        &opts(),
        &f.feed,
        &Version::new(0, 1, 0),
        Channel::Stable,
        None,
    )
    .unwrap()
    .unwrap();
    let dl = download_with(
        &opts().key,
        APP_ID,
        &release,
        &f.downloads,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    let opts_p = PrepareOptions {
        expected_bundle_id: APP_ID.into(),
        current_bundle: Some(f.installed.clone()),
        work_dir: None,
    };
    let mut plan = install(prepare_with(&dl, &opts_p).unwrap(), &f.installed).unwrap();
    plan.backup = BackupDisposal::Delete;
    let outcome = plan.apply().unwrap();
    assert_eq!(outcome.backup, None);
    assert_eq!(
        std::fs::read_dir(f.installed.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    assert_eq!(version_of(&f.installed), "0.2.0");
}

#[test]
fn tampered_zip_is_rejected() {
    let f = fixture();
    let release = check_with(
        &opts(),
        &f.feed,
        &Version::new(0, 1, 0),
        Channel::Stable,
        None,
    )
    .unwrap()
    .unwrap();
    let mut evil = f.zip_bytes.clone();
    let mid = evil.len() / 2;
    evil[mid] ^= 0xff;
    f.server.put("feed/Manta-0.2.0.zip", evil);
    let err = download_with(
        &opts().key,
        APP_ID,
        &release,
        &f.downloads,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(matches!(err, UpdateError::Integrity(_)), "{err}");
    assert!(!f.downloads.join("Manta-0.2.0.zip").exists());
    assert!(!f.downloads.join("Manta-0.2.0.zip.part").exists());

    // Longer than announced.
    let mut longer = f.zip_bytes.clone();
    longer.extend_from_slice(b"trailing");
    f.server.put("feed/Manta-0.2.0.zip", longer);
    let err = download_with(
        &opts().key,
        APP_ID,
        &release,
        &f.downloads,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(matches!(err, UpdateError::Integrity(_)), "{err}");
    assert_eq!(version_of(&f.installed), "0.1.0");
}

#[test]
fn tampered_manifest_is_rejected() {
    let f = fixture();
    let v010 = Version::new(0, 1, 0);
    let good = f.server.state.lock().unwrap().files["feed/update.json"].clone();

    // Edited without re-signing.
    let edited = String::from_utf8(good.clone())
        .unwrap()
        .replace("New things", "Install me");
    f.server.put("feed/update.json", edited.into_bytes());
    let err = check_with(&opts(), &f.feed, &v010, Channel::Stable, None).unwrap_err();
    assert!(matches!(err, UpdateError::Signature(_)), "{err}");

    // Signed with another key.
    let evil = SigningKey::from_bytes(&[1u8; 32]);
    f.server.put("feed/update.json", good.clone());
    f.server.put(
        "feed/update.json.sig",
        keys::sign_manifest(&evil, &good).into_bytes(),
    );
    assert!(matches!(
        check_with(&opts(), &f.feed, &v010, Channel::Stable, None),
        Err(UpdateError::Signature(_))
    ));

    // Missing signature file.
    f.server.remove("feed/update.json.sig");
    let err = check_with(&opts(), &f.feed, &v010, Channel::Stable, None).unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");

    // A validly signed manifest whose release entry was signed by another key.
    let mut r = release_for("0.2.0", &f.zip_bytes, "Manta-0.2.0.zip");
    keys::sign_release(&evil, APP_ID, &mut r).unwrap();
    publish(&f.server, vec![r]);
    assert!(matches!(
        check_with(&opts(), &f.feed, &v010, Channel::Stable, None),
        Err(UpdateError::Signature(_))
    ));

    // Feed for another app.
    let other = CheckOptions {
        app_id: "com.example.other".into(),
        ..opts()
    };
    publish(
        &f.server,
        vec![release_for("0.2.0", &f.zip_bytes, "Manta-0.2.0.zip")],
    );
    assert!(matches!(
        check_with(&other, &f.feed, &v010, Channel::Stable, None),
        Err(UpdateError::Manifest(_))
    ));

    // Unreachable server and 404s give friendly errors.
    let gone = "http://127.0.0.1:9/update.json";
    assert!(matches!(
        check_with(&opts(), gone, &v010, Channel::Stable, None),
        Err(UpdateError::Network(_))
    ));
    let missing = f.server.url("nope/update.json");
    assert!(matches!(
        check_with(&opts(), &missing, &v010, Channel::Stable, None),
        Err(UpdateError::Http { status: 404, .. })
    ));
    // Plain http to a real host is refused before any request.
    assert!(matches!(
        check_with(
            &opts(),
            "http://example.com/update.json",
            &v010,
            Channel::Stable,
            None
        ),
        Err(UpdateError::InsecureUrl(_))
    ));
}

#[test]
fn wrong_bundle_contents_are_rejected() {
    let f = fixture();
    let tmp = tempfile::tempdir().unwrap();
    let cases = [
        ("com.example.evil", "0.2.0", "bundle ID"),
        (APP_ID, "0.3.0", "promised"),
    ];
    for (i, (id, ver, needle)) in cases.iter().enumerate() {
        let dir = tmp.path().join(format!("b{i}"));
        std::fs::create_dir_all(&dir).unwrap();
        let app = make_app(&dir, id, ver);
        let zip = dir.join("a.zip");
        zip_app(&app, &zip);
        let bytes = std::fs::read(&zip).unwrap();
        let name = format!("feed/bad{i}.zip");
        f.server.put(&name, bytes.clone());
        let mut r = release_for("0.2.0", &bytes, &f.server.url(&name));
        r.url = f.server.url(&name);
        let dl_dir = f.downloads.join(format!("bad{i}"));
        let dl = download_with(
            &opts().key,
            APP_ID,
            &r,
            &dl_dir,
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        let err = prepare_with(
            &dl,
            &PrepareOptions {
                expected_bundle_id: APP_ID.into(),
                current_bundle: Some(f.installed.clone()),
                work_dir: None,
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, UpdateError::Bundle(_)) && err.to_string().contains(needle),
            "{err}"
        );
    }

    // A broken code signature (file changed after signing, ZIP re-signed with Ed25519).
    let dir = tmp.path().join("broken");
    std::fs::create_dir_all(&dir).unwrap();
    let app = make_app(&dir, APP_ID, "0.2.0");
    std::fs::write(app.join("Contents/MacOS/manta"), "#!/bin/sh\necho evil\n").unwrap();
    let zip = dir.join("a.zip");
    zip_app(&app, &zip);
    let bytes = std::fs::read(&zip).unwrap();
    f.server.put("feed/broken.zip", bytes.clone());
    let r = release_for("0.2.0", &bytes, &f.server.url("feed/broken.zip"));
    let dl = download_with(
        &opts().key,
        APP_ID,
        &r,
        &f.downloads.join("broken"),
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    let err = prepare_with(
        &dl,
        &PrepareOptions {
            expected_bundle_id: APP_ID.into(),
            current_bundle: None,
            work_dir: None,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("code signature"), "{err}");
    assert_eq!(version_of(&f.installed), "0.1.0");
}

#[test]
fn cancel_mid_download_then_resume() {
    let f = fixture();
    // Make the ZIP big enough to cancel in the middle (incompressible padding file).
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path(), APP_ID, "0.2.0");
    let mut noise = vec![0u8; 600 * 1024];
    let mut x: u32 = 12345;
    for b in noise.iter_mut() {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
        *b = (x >> 16) as u8;
    }
    std::fs::create_dir_all(app.join("Contents/Resources")).unwrap();
    std::fs::write(app.join("Contents/Resources/blob.bin"), &noise).unwrap();
    Command::new("codesign")
        .args(["--force", "-s", "-"])
        .arg(&app)
        .status()
        .unwrap();
    let zip = tmp.path().join("big.zip");
    zip_app(&app, &zip);
    let bytes = std::fs::read(&zip).unwrap();
    f.server.put("feed/Manta-0.2.0.zip", bytes.clone());
    let feed = publish(
        &f.server,
        vec![release_for("0.2.0", &bytes, "Manta-0.2.0.zip")],
    );
    let release = check_with(
        &opts(),
        &feed,
        &Version::new(0, 1, 0),
        Channel::Stable,
        None,
    )
    .unwrap()
    .unwrap();

    f.server.state.lock().unwrap().throttle = Some(Duration::from_millis(20));
    let cancel = AtomicBool::new(false);
    let mut calls = 0;
    let err = download_with(
        &opts().key,
        APP_ID,
        &release,
        &f.downloads,
        &mut |done, _| {
            calls += 1;
            if done > 100 * 1024 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
        &cancel,
    )
    .unwrap_err();
    assert_eq!(err, UpdateError::Cancelled);
    let part = f.downloads.join("Manta-0.2.0.zip.part");
    let have = std::fs::metadata(&part).unwrap().len();
    assert!(
        have > 0 && have < bytes.len() as u64,
        "partial file kept: {have}"
    );
    assert!(calls > 1);

    // Resume: only the rest is requested.
    f.server.state.lock().unwrap().throttle = None;
    let dl = download_with(
        &opts().key,
        APP_ID,
        &release,
        &f.downloads,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(std::fs::read(dl.path()).unwrap(), bytes);
    let log = f.server.state.lock().unwrap().log.clone();
    assert!(
        log.iter()
            .any(|l| l == &format!("feed/Manta-0.2.0.zip range=Some({have})")),
        "{log:?}"
    );
}

#[test]
fn updater_facade_runs_the_whole_flow() {
    let f = fixture();
    let cfg = UpdaterConfig {
        feed_url: f.feed.clone(),
        app_id: APP_ID.into(),
        current_version: Version::new(0, 1, 0),
        channel: Channel::Stable,
        public_key: key().verifying_key(),
        download_dir: f.downloads.clone(),
        bundle_path: Some(f.installed.clone()),
        backup: BackupDisposal::Delete,
        relaunch: false,
        os_version: None,
    };
    let updater = Updater::new(cfg);
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w = wakes.clone();
    updater.set_waker(move || {
        w.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(updater.install_blocker(), None);

    let wait_for = |pred: &dyn Fn(&UpdateStatus) -> bool| {
        for _ in 0..600 {
            let s = updater.status();
            if !updater.is_busy() && pred(&s) {
                return s;
            }
            if let UpdateStatus::Failed(e) = &s {
                panic!("failed: {e}");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("timeout, status {:?}", updater.status());
    };

    assert!(updater.check(None));
    let s = wait_for(&|s| matches!(s, UpdateStatus::Available(_)));
    let UpdateStatus::Available(r) = s else {
        unreachable!()
    };
    assert_eq!(r.version.to_string(), "0.2.0");

    assert!(updater.download());
    wait_for(&|s| matches!(s, UpdateStatus::ReadyToInstall(_)));
    let (_, zip) = updater.downloaded_zip().unwrap();
    assert!(zip.is_file());

    assert!(updater.install_and_relaunch());
    wait_for(&|s| matches!(s, UpdateStatus::Installed { relaunch: false }));
    assert_eq!(version_of(&f.installed), "0.2.0");
    assert!(!zip.exists(), "ZIP removed after install");

    let events = updater.poll();
    assert!(events.contains(&UpdateStatus::Checking));
    assert!(events
        .iter()
        .any(|e| matches!(e, UpdateStatus::Downloading { .. })));
    assert!(events.contains(&UpdateStatus::Installing));
    assert!(wakes.load(Ordering::SeqCst) >= events.len());

    // Checking again from the new version: up to date; skipped versions are honoured.
    let mut cfg2 = updater.config();
    cfg2.current_version = Version::new(0, 2, 0);
    let u2 = Updater::new(cfg2.clone());
    u2.check(None);
    for _ in 0..200 {
        if !u2.is_busy() && u2.status() != UpdateStatus::Idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(u2.status(), UpdateStatus::UpToDate);
    cfg2.current_version = Version::new(0, 1, 0);
    let u3 = Updater::new(cfg2);
    u3.check(Some(Version::new(0, 2, 0)));
    for _ in 0..200 {
        if !u3.is_busy() && u3.status() != UpdateStatus::Idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(u3.status(), UpdateStatus::UpToDate);
}

#[test]
fn install_refuses_read_only_or_missing_targets() {
    let f = fixture();
    let release = check_with(
        &opts(),
        &f.feed,
        &Version::new(0, 1, 0),
        Channel::Stable,
        None,
    )
    .unwrap()
    .unwrap();
    let dl = download_with(
        &opts().key,
        APP_ID,
        &release,
        &f.downloads,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    let p = || {
        prepare_with(
            &dl,
            &PrepareOptions {
                expected_bundle_id: APP_ID.into(),
                current_bundle: None,
                work_dir: None,
            },
        )
        .unwrap()
    };
    let missing = f.installed.with_file_name("Missing.app");
    assert!(matches!(
        install(p(), &missing),
        Err(UpdateError::Install(_))
    ));
    // App Translocation path.
    let tl = Path::new("/private/var/folders/x/T/AppTranslocation/ABC/d/Manta.app");
    assert!(matches!(install(p(), tl), Err(UpdateError::Location(_))));
    // A bare binary (cargo run) has no bundle.
    let exe = f.installed.parent().unwrap().join("manta");
    std::fs::write(&exe, "x").unwrap();
    assert!(matches!(
        install::bundle_for_executable(&exe),
        Err(UpdateError::Location(_))
    ));
    assert_eq!(
        install::bundle_for_executable(&f.installed.join("Contents/MacOS/manta")).unwrap(),
        f.installed.canonicalize().unwrap()
    );
}
