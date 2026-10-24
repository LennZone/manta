//! The SFTP host key dialog: an unknown key is asked about ("Connect and
//! Remember" appends it to known_hosts and connects again), a changed key
//! is refused with a red warning and no way to connect anyway.
//!
//! All tests use one temporary known_hosts file (never `~/.ssh`), each with
//! its own host. `render_host_key_dialogs` writes German light/dark PNGs to
//! `$MANTA_RENDER_DIR`.

use super::*;
use crate::host_key::{HostKeyForm, HostKeyRetry};
use crate::worker::ConnectRequest;
use egui_kittest::kittest::Queryable;
use manta_core::{Auth, ConnectionConfig, HostKeyInfo, Protocol};

/// The known_hosts file of this test binary (set once, process-wide).
fn known_hosts() -> &'static Path {
    static FILE: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    let (_, f) = FILE.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("known_hosts");
        manta_vfs::known_hosts::set_known_hosts_file(Some(f.clone()));
        (dir, f)
    });
    f
}

/// A port nothing listens on (the retry fails fast with "refused").
fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn ed25519(seed: u8) -> Vec<u8> {
    let mut k = Vec::new();
    k.extend_from_slice(&11u32.to_be_bytes());
    k.extend_from_slice(b"ssh-ed25519");
    k.extend_from_slice(&32u32.to_be_bytes());
    k.extend((0..32u8).map(|i| i.wrapping_mul(13).wrapping_add(seed)));
    k
}

fn info(host: &str, port: u16, seed: u8) -> HostKeyInfo {
    manta_vfs::known_hosts::host_key_info(host, port, &ed25519(seed), Some(known_hosts()))
}

fn sftp_config(host: &str, port: u16) -> ConnectionConfig {
    ConnectionConfig {
        name: String::new(),
        protocol: Protocol::Sftp,
        host: host.into(),
        port,
        username: "deploy".into(),
        auth: Auth::Password("pw".into()),
        initial_path: None,
        s3: None,
        container: None,
        ftp: None,
        webdav: None,
        smb: None,
    }
}

/// The connect dialog is waiting for a connection when the key question
/// comes back from the worker (as `poll_worker` would deliver it).
fn ask(h: &mut H, cfg: &ConnectionConfig, info: HostKeyInfo, changed: bool) {
    let a = app(h);
    let mut form = ConnectForm::from_config(cfg, Side::Left, None);
    form.connecting = true;
    a.dialog = Some(Dialog::Connect(form));
    let action = a.actions.next_id();
    let request = ConnectRequest {
        config: cfg.clone(),
        favorite: None,
        side: Side::Left,
        action,
        keychain: false,
    };
    a.on_host_key(info, changed, HostKeyRetry::Connect(Box::new(request)));
    // The modal settles its size over a few frames (wrapped text).
    h.run_steps(6);
}

fn lines_for(pattern: &str) -> usize {
    std::fs::read_to_string(known_hosts())
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with(pattern))
        .count()
}

#[test]
fn unknown_key_is_remembered_and_the_connection_retried() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    let port = closed_port();
    let cfg = sftp_config("127.0.0.1", port);
    ask(&mut h, &cfg, info("127.0.0.1", port, 1), false);
    assert!(matches!(&app(&mut h).dialog, Some(Dialog::HostKey(f)) if !f.changed));
    // Host, key type and fingerprint are shown.
    let fp = manta_vfs::known_hosts::fingerprint(&ed25519(1));
    assert!(h.query_by_label(&fp).is_some(), "fingerprint shown");
    assert!(h.query_by_label("ssh-ed25519").is_some());

    h.get_by_label(t!("hostkey.accept")).click();
    h.run_steps(2);
    let pattern = format!("[127.0.0.1]:{port} ssh-ed25519 ");
    assert_eq!(lines_for(&pattern), 1, "appended to known_hosts");
    // Back in the connect dialog, which connects again: nothing listens on
    // the port, so the retry ends with "refused" (not a key question).
    let start = Instant::now();
    loop {
        h.step();
        match &app(&mut h).dialog {
            Some(Dialog::Connect(f)) if f.connecting => {}
            Some(Dialog::Connect(f)) => {
                let e = f.error.clone().unwrap_or_default();
                assert!(
                    !e.is_empty() && e != t!("hostkey.cancelled") && !e.contains("host key"),
                    "{e}"
                );
                break;
            }
            _ => panic!("the connect dialog comes back"),
        }
        assert!(
            start.elapsed() < crate::test_support::wait(),
            "the retry never finished"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        manta_vfs::known_hosts::check(Some(known_hosts()), "127.0.0.1", port, &ed25519(1)).unwrap(),
        manta_vfs::known_hosts::HostKeyStatus::Known
    );
}

#[test]
fn cancelling_an_unknown_key_writes_nothing() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    let cfg = sftp_config("cancel.invalid", 2201);
    ask(&mut h, &cfg, info("cancel.invalid", 2201, 2), false);
    h.get_by_label(t!("common.cancel")).click();
    h.run_steps(2);
    assert_eq!(lines_for("[cancel.invalid]:2201"), 0);
    match &app(&mut h).dialog {
        Some(Dialog::Connect(f)) => {
            assert!(!f.connecting);
            assert_eq!(f.error.as_deref(), Some(t!("hostkey.cancelled")));
        }
        _ => panic!("the connect dialog comes back"),
    }
}

#[test]
fn a_changed_key_is_refused_without_a_way_around() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    // Known with key 3, the server now shows key 4.
    manta_vfs::known_hosts::trust_host_key(&info("changed.invalid", 22, 3)).unwrap();
    // Other tests share the file: compare this host's lines only.
    let mine = || -> Vec<String> {
        std::fs::read_to_string(known_hosts())
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("changed.invalid "))
            .map(str::to_string)
            .collect()
    };
    let before = mine();
    assert_eq!(before.len(), 1);
    let cfg = sftp_config("changed.invalid", 22);
    ask(&mut h, &cfg, info("changed.invalid", 22, 4), true);
    assert!(matches!(&app(&mut h).dialog, Some(Dialog::HostKey(f)) if f.changed));
    assert!(
        h.query_by_label(t!("hostkey.accept")).is_none(),
        "no 'connect anyway'"
    );
    assert!(
        h.query_by_label_contains("ssh-keygen -R changed.invalid")
            .is_some(),
        "how to fix it"
    );
    // Enter doesn't accept either.
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    match &app(&mut h).dialog {
        Some(Dialog::Connect(f)) => assert_eq!(f.error.as_deref(), Some(t!("hostkey.refused"))),
        _ => panic!("the connect dialog comes back"),
    }
    assert_eq!(mine(), before, "known_hosts untouched");
    // Even a direct accept of the changed key is refused.
    let form = HostKeyForm {
        info: info("changed.invalid", 22, 4),
        changed: false,
        retry: HostKeyRetry::Test,
        back: None,
    };
    app(&mut h).host_key_answered(form, true);
    h.run_steps(2);
    assert!(
        matches!(&app(&mut h).dialog, Some(Dialog::HostKey(f)) if f.changed),
        "turned into the warning"
    );
    assert_eq!(mine(), before);
}

#[test]
fn render_host_key_dialogs() {
    require_gpu!();
    let fx = fixture();
    // As shown to a user: their own known_hosts path.
    let home_kh = dirs::home_dir().unwrap().join(".ssh/known_hosts");
    let shown = |port: u16, seed: u8| {
        manta_vfs::known_hosts::host_key_info(
            "web-01.example.com",
            port,
            &ed25519(seed),
            Some(&home_kh),
        )
    };
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness_de(&fx, WIDE, theme);
        let cfg = sftp_config("web-01.example.com", 22);
        app(&mut h).dialog = Some(Dialog::HostKey(Box::new(HostKeyForm {
            info: shown(22, 7),
            changed: false,
            retry: HostKeyRetry::Test,
            back: Some(Box::new(ConnectForm::from_config(&cfg, Side::Left, None))),
        })));
        save(&mut h, &format!("hostkey_unknown_de_{tname}"));
        app(&mut h).dialog = Some(Dialog::HostKey(Box::new(HostKeyForm {
            info: shown(2222, 8),
            changed: true,
            retry: HostKeyRetry::Test,
            back: None,
        })));
        save(&mut h, &format!("hostkey_changed_de_{tname}"));
    }
    crate::i18n::set_thread_locale(None);
}
