//! `Session::push_run` against a real zsh: the command is typed once the
//! shell is idle (after a cd to its folder), lands in the history, waits
//! while a program runs, and the user's half-typed line survives.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_plugin_terminal::palette::Theme;
use manta_plugin_terminal::session::{GridSize, Session, SpawnOptions};
use manta_plugin_terminal::shim::Integration;

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap()
}

fn wait_until(
    s: &mut Session,
    limit: Duration,
    mut cond: impl FnMut(&mut Session) -> bool,
) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        s.tick(true);
        if cond(s) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

#[test]
fn zsh_runs_commands_when_idle() {
    let shell = "/bin/zsh";
    if !Path::new(shell).exists() {
        eprintln!("skipping: {shell} not installed");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let work_root = tempfile::tempdir().unwrap();
    let work = work_root.path().join("it's a dir ü");
    std::fs::create_dir(&work).unwrap();
    let out = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    // An empty ZDOTDIR: the user's own rc files stay out of this test.
    let zd = tempfile::tempdir().unwrap();

    let mut s = Session::spawn(SpawnOptions {
        shell: PathBuf::from(shell),
        cwd: home.path().to_path_buf(),
        size: GridSize {
            cols: 120,
            rows: 30,
        },
        cell_px: (8, 16),
        scrollback: 1000,
        shim_root: data.path().to_path_buf(),
        theme: Theme::dark(),
        waker: Arc::new(|| {}),
        spool: None,
        zdotdir: Some(zd.path().to_path_buf()),
        remote: None,
    })
    .expect("spawn");
    assert!(
        wait_until(&mut s, Duration::from_secs(20), |s| s.is_idle()),
        "no idle prompt. Screen:\n{}",
        s.lock().screen_text()
    );
    assert_eq!(s.integration(), Some(Integration::Push));

    // The user has half-typed a command.
    s.write(b"echo kee");
    std::thread::sleep(Duration::from_millis(200));

    // Run in another folder: cd there first, then type the command.
    let marker = out.path().join("pwd.txt");
    let cmd = format!("pwd -P > '{}'", marker.display());
    s.push_run(&cmd, Some(&work));
    assert_eq!(s.runs_waiting(), 1);
    assert!(
        wait_until(&mut s, Duration::from_secs(8), |_| !read(&marker)
            .is_empty()),
        "command didn't run. Screen:\n{}",
        s.lock().screen_text()
    );
    assert_eq!(read(&marker).trim(), canon(&work).to_string_lossy());
    assert_eq!(s.runs_waiting(), 0);
    // Our cd's echo doesn't navigate the pane.
    assert_eq!(s.tick(true), None);

    // The parked line comes back: finish it.
    assert!(wait_until(&mut s, Duration::from_secs(5), |s| s.is_idle()));
    std::thread::sleep(Duration::from_millis(300));
    s.write(b"p_me\r");
    assert!(
        wait_until(&mut s, Duration::from_secs(5), |s| s
            .lock()
            .screen_text()
            .lines()
            .any(|l| l.trim() == "keep_me")),
        "typed input was lost. Screen:\n{}",
        s.lock().screen_text()
    );

    // Busy: the command waits until the program has finished.
    assert!(wait_until(&mut s, Duration::from_secs(5), |s| s.is_idle()));
    s.write(b"sleep 1.2\r");
    s.user_submitted();
    let queued = out.path().join("queued.txt");
    s.push_run(&format!("echo QUEUED > '{}'", queued.display()), None);
    let start = Instant::now();
    wait_until(&mut s, Duration::from_millis(600), |_| false);
    assert!(!queued.exists(), "ran while the shell was busy");
    assert_eq!(s.runs_waiting(), 1);
    assert!(
        wait_until(&mut s, Duration::from_secs(6), |_| read(&queued).trim()
            == "QUEUED"),
        "queued command didn't run. Screen:\n{}",
        s.lock().screen_text()
    );
    assert!(start.elapsed() >= Duration::from_millis(900));

    // Typed commands land in the history (no leading space).
    let hist = out.path().join("hist.txt");
    assert!(wait_until(&mut s, Duration::from_secs(5), |s| s.is_idle()));
    s.push_run(&format!("fc -ln -10 > '{}'", hist.display()), None);
    assert!(wait_until(&mut s, Duration::from_secs(5), |_| read(&hist)
        .contains("QUEUED")));
    let h = read(&hist);
    assert!(h.contains("echo QUEUED"), "{h}");
    assert!(h.contains("pwd -P"), "{h}");
    assert!(
        !h.contains(" cd -- "),
        "the follow cd stays out of the history: {h}"
    );

    s.shutdown(Duration::from_millis(500));
}
