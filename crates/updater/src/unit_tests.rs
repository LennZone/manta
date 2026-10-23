//! Unit tests for the pure parts (no network, no bundles).

use chrono::{Duration, TimeZone, Utc};
use ed25519_dalek::SigningKey;
use semver::Version;

use crate::bundle::{check_team_policy, parse_codesign_display, Signing};
use crate::check::{resolve_url, select_release, signature_url, validate_url, OsVersion};
use crate::install::{
    admin_script, bundle_of_executable, check_location, shell_quote, LocationProblem,
};
use crate::keys;
use crate::manifest::{Channel, Manifest, Notes, Release, SCHEMA};
use crate::schedule::{should_check, DownloadedUpdate, UpdateState};
use crate::UpdateError;

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn release(v: &str) -> Release {
    let mut r = Release {
        version: Version::parse(v).unwrap(),
        channel: None,
        published: Utc
            .with_ymd_and_hms(2026, 10, 1, 12, 0, 0)
            .unwrap()
            .fixed_offset(),
        min_macos: "11.0".into(),
        notes: Notes {
            en: "Fixes".into(),
            de: Some("Korrekturen".into()),
        },
        url: format!("Manta-{v}.zip"),
        size: 1234,
        sha256: "ab".repeat(32),
        signature: String::new(),
    };
    keys::sign_release(&key(), "com.manta-filemanager.manta", &mut r).unwrap();
    r
}

fn manifest(versions: &[&str]) -> Manifest {
    Manifest {
        schema: SCHEMA,
        app_id: "com.manta-filemanager.manta".into(),
        channel: Channel::Stable,
        generated: None,
        releases: versions.iter().map(|v| release(v)).collect(),
    }
}

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

// ---------------------------------------------------------------- manifest

#[test]
fn manifest_roundtrip_and_validation() {
    let m = manifest(&["0.2.0", "0.1.1"]);
    let json = m.to_json();
    let back = Manifest::parse(json.as_bytes()).unwrap();
    assert_eq!(back, m);

    let mut bad = m.clone();
    bad.releases[0].sha256 = "XYZ".into();
    assert!(matches!(
        Manifest::parse(bad.to_json().as_bytes()),
        Err(UpdateError::Manifest(_))
    ));

    let mut dup = m.clone();
    dup.releases.push(dup.releases[0].clone());
    assert!(Manifest::parse(dup.to_json().as_bytes())
        .unwrap_err()
        .to_string()
        .contains("twice"));

    let mut no_notes = m.clone();
    no_notes.releases[0].notes.en = " ".into();
    assert!(Manifest::parse(no_notes.to_json().as_bytes()).is_err());

    let mut bad_os = m.clone();
    bad_os.releases[0].min_macos = "eleven".into();
    assert!(Manifest::parse(bad_os.to_json().as_bytes()).is_err());

    let mut zero = m.clone();
    zero.releases[0].size = 0;
    assert!(Manifest::parse(zero.to_json().as_bytes()).is_err());

    assert!(Manifest::parse(b"not json").is_err());
    assert!(Manifest::parse(br#"{"app_id":"x","releases":[]}"#)
        .unwrap_err()
        .to_string()
        .contains("schema"));
    let future = Manifest::parse(br#"{"schema":2,"app_id":"x","releases":[]}"#).unwrap_err();
    assert!(future.to_string().contains("newer Manta"), "{future}");
}

#[test]
fn manifest_accepts_unknown_fields_and_documented_example() {
    let r = release("1.0.0");
    let json = format!(
        r#"{{"schema":1,"app_id":"com.manta-filemanager.manta","channel":"stable","future_field":true,
            "releases":[{{"version":"1.0.0","published":"2026-10-01T12:00:00+02:00","min_macos":"11.0",
            "notes":{{"en":"Hi"}},"url":"https://example.com/a.zip","size":1234,
            "sha256":"{}","signature":"{}","extra":1}}]}}"#,
        r.sha256, r.signature
    );
    let m = Manifest::parse(json.as_bytes()).unwrap();
    assert_eq!(m.releases[0].notes.de, None);
    assert_eq!(m.releases[0].notes.for_language("de-DE"), "Hi");
}

#[test]
fn notes_language_fallback() {
    let n = Notes {
        en: "en".into(),
        de: Some("de".into()),
    };
    assert_eq!(n.for_language("de"), "de");
    assert_eq!(n.for_language("DE_at"), "de");
    assert_eq!(n.for_language("fr"), "en");
}

#[test]
fn effective_channel_from_prerelease() {
    assert_eq!(release("1.0.0").effective_channel(), Channel::Stable);
    assert_eq!(release("1.1.0-beta.1").effective_channel(), Channel::Beta);
    let mut r = release("1.2.0");
    r.channel = Some(Channel::Beta);
    assert_eq!(r.effective_channel(), Channel::Beta);
}

// ---------------------------------------------------------------- signatures

#[test]
fn release_signature_valid() {
    let r = release("0.2.0");
    keys::verify_release(&key().verifying_key(), "com.manta-filemanager.manta", &r).unwrap();
}

#[test]
fn release_signature_rejects_tampering_and_replay() {
    let vk = key().verifying_key();
    let good = release("0.2.0");

    let mut sha = good.clone();
    sha.sha256 = "cd".repeat(32);
    assert!(matches!(
        keys::verify_release(&vk, "com.manta-filemanager.manta", &sha),
        Err(UpdateError::Signature(_))
    ));

    let mut size = good.clone();
    size.size += 1;
    assert!(keys::verify_release(&vk, "com.manta-filemanager.manta", &size).is_err());

    // Replay: the signature of 0.2.0 on another version.
    let mut replay = release("0.3.0");
    replay.signature = good.signature.clone();
    assert!(keys::verify_release(&vk, "com.manta-filemanager.manta", &replay).is_err());

    // Another app's signature.
    assert!(keys::verify_release(&vk, "com.example.other", &good).is_err());

    // Relabelled channel / min_macos.
    let mut chan = good.clone();
    chan.channel = Some(Channel::Beta);
    assert!(keys::verify_release(&vk, "com.manta-filemanager.manta", &chan).is_err());
    let mut os = good.clone();
    os.min_macos = "10.15".into();
    assert!(keys::verify_release(&vk, "com.manta-filemanager.manta", &os).is_err());

    // Wrong key.
    let other = SigningKey::from_bytes(&[9u8; 32]).verifying_key();
    assert!(keys::verify_release(&other, "com.manta-filemanager.manta", &good).is_err());

    // Garbage signature.
    let mut garbage = good.clone();
    garbage.signature = "!!".into();
    assert!(keys::verify_release(&vk, "com.manta-filemanager.manta", &garbage).is_err());

    // URL and notes are not bound (mirrors); the manifest signature covers them.
    let mut mirror = good.clone();
    mirror.url = "https://mirror.example/x.zip".into();
    keys::verify_release(&vk, "com.manta-filemanager.manta", &mirror).unwrap();
}

#[test]
fn release_message_is_unambiguous() {
    let mut r = release("0.2.0");
    r.min_macos = "11.0\nsize=1".into();
    assert!(
        keys::verify_release(&key().verifying_key(), "com.manta-filemanager.manta", &r).is_err()
    );
    assert!(keys::sign_release(&key(), "com.manta-filemanager.manta", &mut r).is_err());
}

#[test]
fn manifest_signature() {
    let json = manifest(&["0.2.0"]).to_json();
    let sig = keys::sign_manifest(&key(), json.as_bytes());
    let vk = key().verifying_key();
    keys::verify_manifest(&vk, json.as_bytes(), &sig).unwrap();
    let tampered = json.replace("Fixes", "Fixez");
    assert!(keys::verify_manifest(&vk, tampered.as_bytes(), &sig).is_err());
    let other = SigningKey::from_bytes(&[9u8; 32]).verifying_key();
    assert!(keys::verify_manifest(&other, json.as_bytes(), &sig).is_err());
    // A release signature can't stand in for the manifest signature.
    let r = release("0.2.0");
    assert!(keys::verify_manifest(&vk, json.as_bytes(), &r.signature).is_err());
}

#[test]
fn key_files_roundtrip() {
    let k = keys::generate_private_key().unwrap();
    let priv_text = keys::private_key_file_contents(&k);
    let pub_text = keys::public_key_file_contents(&k.verifying_key());
    assert!(priv_text.starts_with('#'));
    assert_eq!(
        keys::parse_private_key(&priv_text).unwrap().to_bytes(),
        k.to_bytes()
    );
    assert_eq!(
        keys::parse_public_key(&pub_text).unwrap(),
        k.verifying_key()
    );
    assert!(keys::parse_public_key("AAAA").is_err());
    assert_ne!(
        keys::generate_private_key().unwrap().to_bytes(),
        k.to_bytes()
    );
}

// ---------------------------------------------------------------- selection

#[test]
fn selects_newest_newer_release() {
    let m = manifest(&["0.1.0", "0.3.0", "0.2.0"]);
    let r = select_release(&m, &v("0.1.0"), Channel::Stable, None, None).unwrap();
    assert_eq!(r.version, v("0.3.0"));
    assert!(select_release(&m, &v("0.3.0"), Channel::Stable, None, None).is_none());
    assert!(
        select_release(&m, &v("1.0.0"), Channel::Stable, None, None).is_none(),
        "never downgrade"
    );
}

#[test]
fn channels() {
    let m = manifest(&["0.2.0", "0.3.0-beta.1"]);
    assert_eq!(
        select_release(&m, &v("0.1.0"), Channel::Stable, None, None)
            .unwrap()
            .version,
        v("0.2.0")
    );
    assert_eq!(
        select_release(&m, &v("0.1.0"), Channel::Beta, None, None)
            .unwrap()
            .version,
        v("0.3.0-beta.1")
    );
    // A beta user on 0.3.0-beta.1 gets the final 0.3.0 once it is out.
    let m2 = manifest(&["0.3.0", "0.3.0-beta.1"]);
    assert_eq!(
        select_release(&m2, &v("0.3.0-beta.1"), Channel::Beta, None, None)
            .unwrap()
            .version,
        v("0.3.0")
    );
    assert_eq!(
        select_release(&m2, &v("0.3.0-beta.1"), Channel::Stable, None, None)
            .unwrap()
            .version,
        v("0.3.0")
    );
}

#[test]
fn min_macos_picks_newest_compatible() {
    let mut m = manifest(&["0.2.0", "0.3.0"]);
    m.releases[1].min_macos = "15.0".into();
    let old_os = OsVersion::parse("13.6.1").unwrap();
    let new_os = OsVersion::parse("15").unwrap();
    assert_eq!(
        select_release(&m, &v("0.1.0"), Channel::Stable, None, Some(&old_os))
            .unwrap()
            .version,
        v("0.2.0")
    );
    assert_eq!(
        select_release(&m, &v("0.1.0"), Channel::Stable, None, Some(&new_os))
            .unwrap()
            .version,
        v("0.3.0")
    );
    assert!(select_release(&m, &v("0.2.0"), Channel::Stable, None, Some(&old_os)).is_none());
}

#[test]
fn skipped_version() {
    let m = manifest(&["0.2.0", "0.3.0"]);
    assert!(select_release(&m, &v("0.1.0"), Channel::Stable, Some(&v("0.3.0")), None).is_none());
    let m2 = manifest(&["0.2.0", "0.3.0", "0.4.0"]);
    assert_eq!(
        select_release(&m2, &v("0.1.0"), Channel::Stable, Some(&v("0.3.0")), None)
            .unwrap()
            .version,
        v("0.4.0")
    );
}

#[test]
fn os_versions() {
    assert_eq!(OsVersion::parse("11.0"), OsVersion::parse("11"));
    assert!(OsVersion::parse("10.15.7") < OsVersion::parse("11.0"));
    assert!(OsVersion::parse("14.10") > OsVersion::parse("14.9"));
    assert!(OsVersion::parse("").is_none());
    assert!(OsVersion::parse("11.x").is_none());
    assert!(OsVersion::parse("-1").is_none());
    assert!(crate::check::macos_version().is_some());
}

// ---------------------------------------------------------------- URLs

#[test]
fn https_only() {
    assert!(validate_url("https://example.com/manta/update.json").is_ok());
    assert!(validate_url("HTTPS://Example.com").is_ok());
    assert!(validate_url("http://127.0.0.1:8000/update.json").is_ok());
    assert!(validate_url("http://localhost/update.json").is_ok());
    assert!(validate_url("http://[::1]:9/x").is_ok());
    for bad in [
        "http://example.com/update.json",
        "http://127.0.0.1.evil.com/x",
        "http://localhost.evil.com/x",
        "http://evil.com@127.0.0.1/x",
        "https://user@example.com/x",
        "ftp://example.com/x",
        "file:///etc/passwd",
        "example.com/update.json",
        "https:///nohost",
        "https://exa mple.com",
    ] {
        assert!(validate_url(bad).is_err(), "{bad} must be refused");
    }
    assert!(matches!(
        validate_url("http://example.com"),
        Err(UpdateError::InsecureUrl(_))
    ));
}

#[test]
fn url_resolution() {
    let feed = "https://example.com/manta/update.json";
    assert_eq!(
        resolve_url(feed, "Manta-1.0.0.zip"),
        "https://example.com/manta/Manta-1.0.0.zip"
    );
    assert_eq!(
        resolve_url(feed, "/dl/a.zip"),
        "https://example.com/dl/a.zip"
    );
    assert_eq!(
        resolve_url(feed, "https://cdn.example/a.zip"),
        "https://cdn.example/a.zip"
    );
    assert_eq!(
        resolve_url("https://example.com", "a.zip"),
        "https://example.com/a.zip"
    );
    assert_eq!(
        resolve_url("https://example.com/u.json?x=1", "a.zip"),
        "https://example.com/a.zip"
    );
    assert_eq!(
        signature_url(feed),
        "https://example.com/manta/update.json.sig"
    );
    assert_eq!(
        signature_url("https://e.com/u.json?v=2"),
        "https://e.com/u.json.sig?v=2"
    );
}

// ---------------------------------------------------------------- install locations

#[test]
fn location_checks() {
    use std::path::Path;
    let exe = Path::new("/Applications/Manta.app/Contents/MacOS/manta");
    let app = bundle_of_executable(exe).unwrap();
    assert_eq!(app, Path::new("/Applications/Manta.app"));
    assert_eq!(check_location(Some(&app), false).unwrap(), app);

    // cargo run
    assert_eq!(
        bundle_of_executable(Path::new("/Users/me/manta/target/debug/manta")),
        None
    );
    assert_eq!(
        check_location(None, false),
        Err(LocationProblem::NotBundled)
    );
    assert_eq!(
        bundle_of_executable(Path::new("/x/Contents/MacOS/manta")),
        None
    );

    // DMG
    let dmg = Path::new("/Volumes/Manta/Manta.app");
    assert_eq!(
        check_location(Some(dmg), true),
        Err(LocationProblem::ReadOnlyVolume)
    );
    // A writable external disk is fine.
    assert!(check_location(Some(Path::new("/Volumes/Data/Apps/Manta.app")), false).is_ok());

    // App Translocation
    let tl = Path::new("/private/var/folders/xy/T/AppTranslocation/1234-ABCD/d/Manta.app");
    assert_eq!(
        check_location(Some(tl), false),
        Err(LocationProblem::Translocated)
    );
    for p in [
        LocationProblem::NotBundled,
        LocationProblem::Translocated,
        LocationProblem::ReadOnlyVolume,
    ] {
        assert!(p.to_string().len() > 30);
    }
    // The real root volume is read-only (sealed system volume), the temp dir is not.
    assert!(!crate::install::is_read_only(&std::env::temp_dir()));
}

#[test]
fn admin_script_quotes_paths() {
    use std::path::Path;
    let s = admin_script(
        Path::new("/tmp/it's here/Manta.app"),
        Path::new("/Applications/Manta.app"),
        Path::new("/Applications/.Manta.app.update-1"),
        Path::new("/Applications/.Manta.app.old-1"),
    )
    .unwrap();
    assert!(s.contains(r"'/tmp/it'\''s here/Manta.app'"), "{s}");
    assert!(s.contains("/bin/mv '/Applications/Manta.app' '/Applications/.Manta.app.old-1'"));
    assert!(s.contains("else /bin/mv '/Applications/.Manta.app.old-1' '/Applications/Manta.app'"));
    assert!(admin_script(
        Path::new("rel"),
        Path::new("/a"),
        Path::new("/b"),
        Path::new("/c")
    )
    .is_err());
    assert!(admin_script(
        Path::new("/a\nrm -rf /"),
        Path::new("/a"),
        Path::new("/b"),
        Path::new("/c")
    )
    .is_err());
    assert_eq!(shell_quote(Path::new("/a b")), "'/a b'");
}

// ---------------------------------------------------------------- code signing

#[test]
fn codesign_parsing_and_team_policy() {
    let adhoc = "Executable=/x\nIdentifier=com.manta-filemanager.manta\nFormat=app bundle with Mach-O thin (arm64)\nCodeDirectory v=20400 size=1 flags=0x2(adhoc) hashes=1+7\nSignature=adhoc\nTeamIdentifier=not set\n";
    let team = "Identifier=com.manta-filemanager.manta\nAuthority=Developer ID Application: Jane (AB12CD34EF)\nTeamIdentifier=AB12CD34EF\n";
    let other = "TeamIdentifier=ZZZZZZZZZZ\n";
    assert_eq!(parse_codesign_display(adhoc), Signing::AdHoc);
    assert_eq!(
        parse_codesign_display(team),
        Signing::Identity {
            team_id: Some("AB12CD34EF".into())
        }
    );
    assert_eq!(
        parse_codesign_display("/x: code object is not signed at all"),
        Signing::Unsigned
    );

    let t = parse_codesign_display(team);
    check_team_policy(&t, &t).unwrap();
    assert!(check_team_policy(&t, &parse_codesign_display(other)).is_err());
    assert!(check_team_policy(&t, &Signing::AdHoc).is_err());
    check_team_policy(&Signing::AdHoc, &Signing::AdHoc).unwrap();
    check_team_policy(&Signing::AdHoc, &t).unwrap();
    assert!(check_team_policy(&Signing::AdHoc, &Signing::Unsigned).is_err());
}

// ---------------------------------------------------------------- scheduling

#[test]
fn scheduling() {
    let now = Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap();
    let day = Duration::hours(24);
    assert!(should_check(None, now, day));
    assert!(!should_check(Some(now - Duration::hours(23)), now, day));
    assert!(should_check(Some(now - Duration::hours(24)), now, day));
    assert!(
        !should_check(Some(now + Duration::hours(2)), now, day),
        "small clock skew"
    );
    assert!(
        should_check(Some(now + Duration::days(3)), now, day),
        "clock went back"
    );

    // Default: every launch.
    let mut st = UpdateState::default();
    assert_eq!(st.interval_hours, 0);
    assert!(st.check_due(now));
    st.last_check = Some(now);
    assert!(st.check_due(now), "every launch, even right after a check");
    st.interval_hours = 24;
    assert!(st.check_due(now + Duration::hours(25)));
    assert!(!st.check_due(now + Duration::hours(1)));
    st.auto_check = false;
    assert!(!st.check_due(now + Duration::days(10)));
}

#[test]
fn state_serde_and_stale_download() {
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("Manta-0.2.0.zip");
    std::fs::write(&zip, b"x").unwrap();
    let mut st = UpdateState {
        skipped_version: Some(v("0.1.5")),
        downloaded: Some(DownloadedUpdate {
            release: release("0.2.0"),
            zip: zip.clone(),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&st).unwrap();
    assert_eq!(serde_json::from_str::<UpdateState>(&json).unwrap(), st);
    // Old/partial JSON still loads.
    let partial: UpdateState = serde_json::from_str(r#"{"channel":"beta"}"#).unwrap();
    assert_eq!(partial.channel, Channel::Beta);
    assert!(partial.auto_check);

    assert!(!st.forget_stale_download(&v("0.1.0")));
    assert!(st.forget_stale_download(&v("0.2.0")), "installed now");
    assert!(st.downloaded.is_none());
    assert!(!zip.exists());
}
