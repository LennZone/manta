use super::*;

fn info() -> SystemInfo {
    SystemInfo {
        manta: "0.3.6".into(),
        macos: "macOS 15.6.1".into(),
        arch: "Apple silicon (arm64)".into(),
    }
}

fn frames() -> Vec<Frame> {
    vec![
        Frame {
            addr: 0x1_0000_4f20,
            image: Some("manta".into()),
            offset: Some(0x4f20),
            symbol: None,
        },
        Frame {
            addr: 0x1_8a2b_3c4d,
            image: Some("libsystem_kernel.dylib".into()),
            offset: Some(0x3c4d),
            symbol: Some("__pthread_kill".into()),
        },
    ]
}

fn image() -> Image {
    Image {
        name: "manta".into(),
        uuid: Some("2F9C3A1E-0B5D-3C7E-9A11-5E2F3D4C5B6A".into()),
        base: 0x1_0000_0000,
        text_size: 0x10_0000,
    }
}

#[test]
fn scrub_removes_personal_data() {
    let sens = vec!["web-01.internal".to_string(), "deploy".to_string()];
    let s = scrub(
        "can't read /Users/anna/Documents/a.txt on deploy@web-01.internal; \
         sftp://bob@files.example.com:22/srv, mail anna@example.com, \
         /Users/other/x and /Users/Shared/y, host web-01.internal",
        Some("/Users/anna"),
        &sens,
    );
    assert!(s.contains("~/Documents/a.txt"), "{s}");
    assert!(s.contains("sftp://<host>:22/srv"), "{s}");
    assert!(s.contains("<user>@<host>"), "{s}");
    assert!(s.contains("/Users/<user>/x"), "{s}");
    assert!(s.contains("/Users/Shared/y"), "{s}");
    for gone in ["anna", "bob", "deploy", "web-01", "example.com", "other"] {
        assert!(!s.contains(gone), "{gone} left in: {s}");
    }
    // Ordinary text stays.
    assert_eq!(
        scrub(
            "index out of bounds: the len is 3 but the index is 7",
            None,
            &[]
        ),
        "index out of bounds: the len is 3 but the index is 7"
    );
    assert_eq!(
        scrub("crates/app/src/pane.rs:12:5", Some("/Users/anna"), &[]),
        "crates/app/src/pane.rs:12:5"
    );
}

#[test]
fn stamps_sort_by_time() {
    assert_eq!(utc_stamp(0, false), "1970-01-01-000000");
    assert_eq!(utc_stamp(1_792_843_501, true), "2026-10-24 12:05:01");
    assert_eq!(utc_stamp(1_792_843_501, false), "2026-10-24-120501");
    assert_eq!(utc_stamp(951_782_400, true), "2000-02-29 00:00:00");
    assert!(utc_stamp(1_792_843_501, false) > utc_stamp(1_792_843_500, false));
}

#[test]
fn report_has_versions_binary_and_frames() {
    let fr = frames();
    let text = report_text(
        &info(),
        Some(&image()),
        1_792_843_501,
        &Crash {
            kind: "panic",
            thread: "main",
            message: "index out of bounds",
            location: Some("crates/app/src/pane.rs:12:5"),
            frames: &fr,
        },
    );
    for want in [
        "Manta: 0.3.6",
        "macOS: macOS 15.6.1",
        "Architecture: Apple silicon (arm64)",
        "Time: 2026-10-24 12:05:01 UTC",
        "Kind: panic",
        "Message: index out of bounds",
        "Location: crates/app/src/pane.rs:12:5",
        "UUID: 2F9C3A1E-0B5D-3C7E-9A11-5E2F3D4C5B6A",
        "Load address: 0x100000000",
        "  0  0x0000000100004f20  manta + 0x4f20",
        "  1  0x000000018a2b3c4d  libsystem_kernel.dylib + 0x3c4d  __pthread_kill",
        "Resources/DWARF/manta -l 0x100000000 0x100004f20\n",
    ] {
        assert!(text.contains(want), "{want:?} missing in\n{text}");
    }
    let sum = summary(&text);
    assert!(sum.starts_with("Kind: panic\nMessage: index out of bounds\n"));
    assert!(sum.contains("First frames:\n  0  0x0000000100004f20  manta + 0x4f20"));
    assert!(!sum.contains("atos"), "{sum}");
}

#[test]
fn uuid_format() {
    let b: Vec<u8> = (0u8..16).collect();
    assert_eq!(format_uuid(&b), "00010203-0405-0607-0809-0A0B0C0D0E0F");
}

#[test]
fn the_running_binary_has_a_uuid() {
    #[cfg(target_os = "macos")]
    {
        let img = main_image().expect("main image");
        let uuid = img.uuid.expect("LC_UUID");
        assert_eq!(uuid.len(), 36, "{uuid}");
        assert!(img.text_size > 0);
        // Our own code lies in __TEXT.
        let f = the_running_binary_has_a_uuid as *const () as usize;
        assert!(f >= img.base && f < img.base + img.text_size);
    }
}

#[test]
fn pending_reports_are_shown_once() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    assert_eq!(pending(dir), None);
    let a = write_report(dir, 1_792_843_501, "a").unwrap();
    let b = write_report(dir, 1_792_843_501, "b").unwrap();
    assert_ne!(a, b, "same second: a second name");
    assert_eq!(pending(dir).as_deref(), Some(b.as_path()));
    mark_seen(dir, &b);
    assert_eq!(pending(dir), None, "once");
    let c = write_report(dir, 1_792_843_600, "c").unwrap();
    assert_eq!(pending(dir).as_deref(), Some(c.as_path()), "a newer one");
    for i in 0..20 {
        write_report(dir, 1_800_000_000 + i, "x").unwrap();
    }
    assert_eq!(reports(dir).len(), KEEP, "old ones are pruned");
}

#[test]
fn github_url_is_prefilled_and_short() {
    let fr = frames();
    let text = report_text(
        &info(),
        Some(&image()),
        1_792_843_501,
        &Crash {
            kind: "panic",
            thread: "main",
            message: "called `Option::unwrap()` on a `None` value",
            location: Some("crates/app/src/pane.rs:12:5"),
            frames: &fr,
        },
    );
    let url = issue_url(&info(), "crash-2026-10-24-120501.txt", &text);
    assert!(url.starts_with(
        "https://github.com/web-workers-com/manta/issues/new?template=bug_report.yml\
         &manta-version=0.3.6&macos-version=macOS%2015.6.1"
    ));
    assert!(url.contains("&title=Crash%3A%20called%20%60Option%3A%3Aunwrap"));
    assert!(url.contains("&actual=Manta%20quit%20unexpectedly."));
    assert!(url.contains("manta%20%2B%200x4f20"), "first frames: {url}");
    assert!(
        url.contains("crash-2026-10-24-120501.txt"),
        "names the file"
    );

    // A huge report still gives a URL under 8 kB.
    let many: Vec<Frame> = (0..64)
        .map(|i| Frame {
            addr: 0x1_0000_0000 + i * 0x100,
            image: Some("libsomething-with-a-very-long-name.dylib".into()),
            offset: Some(i * 0x100),
            symbol: Some("_ZN4core9panicking5panic17h0123456789abcdefE".repeat(4)),
        })
        .collect();
    let long_msg = "ü".repeat(3000);
    let text = report_text(
        &info(),
        Some(&image()),
        0,
        &Crash {
            kind: "panic",
            thread: "main",
            message: &long_msg,
            location: None,
            frames: &many,
        },
    );
    let url = issue_url(&info(), "crash-x.txt", &text);
    assert!(url.len() <= crate::feedback::MAX_URL, "{}", url.len());
    assert!(url.contains("&actual=Manta%20quit%20unexpectedly."));
}

/// The child processes of the tests below: they install the real hooks
/// with a temporary HOME and crash. Does nothing in a normal test run.
#[test]
fn child_process() {
    let Ok(mode) = std::env::var("MANTA_CRASH_CHILD") else {
        return;
    };
    let dir = dir().expect("log dir under the temp HOME");
    install(dir);
    note_sensitive("web-01.internal");
    let home = std::env::var("HOME").unwrap();
    match mode.as_str() {
        "panic" => panic!(
            "can't open {home}/Documents/secret.txt on deploy@web-01.internal \
             (sftp://anna@files.example.com/srv)"
        ),
        "abort" => std::process::abort(),
        "nounwind" => {
            extern "C" fn callback() {
                panic!("inside a callback");
            }
            callback();
        }
        _ => {}
    }
}

/// Run [`child_process`] in `mode` with a temporary HOME; its reports.
fn crash_child(
    mode: &str,
) -> (
    std::process::Output,
    tempfile::TempDir,
    Vec<(String, String)>,
) {
    let home = tempfile::tempdir().unwrap();
    let home_path = home.path().canonicalize().unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "crash::tests::child_process",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("MANTA_CRASH_CHILD", mode)
        .env("HOME", &home_path)
        .output()
        .unwrap();
    let logs = home_path.join("Library/Logs/Manta");
    let reports = reports(&logs)
        .into_iter()
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    (out, home, reports)
}

#[test]
fn a_panic_writes_a_scrubbed_report() {
    let (out, home, reports) = crash_child("panic");
    assert!(!out.status.success(), "the child panicked");
    assert_eq!(reports.len(), 1, "{reports:?}");
    let (name, text) = &reports[0];
    assert!(
        name.starts_with("crash-") && name.ends_with(".txt"),
        "{name}"
    );
    assert!(text.contains(&format!("Manta: {}", env!("CARGO_PKG_VERSION"))));
    assert!(text.contains("macOS: macOS ") || !cfg!(target_os = "macos"));
    assert!(text.contains("Kind: panic"));
    assert!(text.contains("Message: can't open ~/Documents/secret.txt on <user>@<redacted>"));
    assert!(text.contains("(sftp://<host>/srv)"), "{text}");
    assert!(
        text.contains("Location: crates/app/src/crash/tests.rs:"),
        "{text}"
    );
    #[cfg(target_os = "macos")]
    {
        assert!(text.contains("\nUUID: ") && !text.contains("UUID: unknown"));
        assert!(text.contains("\nBacktrace:\n  0  0x"), "{text}");
    }
    let home = home.path().canonicalize().unwrap();
    for leak in [
        home.to_string_lossy().as_ref(),
        "web-01",
        "deploy",
        "anna",
        "example.com",
    ] {
        assert!(!text.contains(leak), "{leak} in\n{text}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn an_abort_writes_a_signal_report() {
    let (out, _home, reports) = crash_child("abort");
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(out.status.signal(), Some(libc::SIGABRT), "{out:?}");
    assert_eq!(reports.len(), 1, "{reports:?}");
    let (name, text) = &reports[0];
    assert!(name.ends_with("-signal.txt"), "{name}");
    assert!(text.contains("Kind: signal SIGABRT"), "{text}");
    assert!(text.contains(&format!("Manta: {}", env!("CARGO_PKG_VERSION"))));
    assert!(
        text.contains("\nUUID: ") && text.contains("\nBacktrace:\n"),
        "{text}"
    );
    // Unwinding from a signal handler works on current macOS but not on every
    // version (macOS 14 CI runners give no frames): frames are best effort.
    if text.contains("\nBacktrace:\n  0  0x") {
        assert!(text.contains(" + 0x"), "frames in our binary: {text}");
    } else {
        assert!(text.contains("(no frames available)"), "{text}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_panic_that_aborts_writes_one_report() {
    let (out, _home, reports) = crash_child("nounwind");
    assert!(!out.status.success());
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].1.contains("Message: inside a callback"));
}
