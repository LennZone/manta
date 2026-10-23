//! Right prompts (zsh RPROMPT) must stay right-aligned at every width, on
//! the current prompt AND on history lines, with no fragments after the
//! panel shrinks or grows. Driven through a real zsh in a PTY, once with a
//! hermetic config and once with the user's own (~/.zshrc, e.g. p10k).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;
use manta_core::VfsPath;
use manta_plugin::{FileAction, FileActionKind, FileActionPhase, PaneId};
use manta_plugin_terminal::palette::Theme;
use manta_plugin_terminal::session::{GridSize, Session, SpawnOptions};

const ROWS: u16 = 24;

fn pump(s: &mut Session, ms: u64) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        s.tick(true);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait(s: &mut Session, limit: Duration, cond: impl Fn(&Session) -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < limit {
        s.tick(true);
        if cond(s) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

struct Env {
    s: Session,
    cwd: PathBuf,
    other: PathBuf,
    _dirs: Vec<tempfile::TempDir>,
}

fn spawn(cols: u16, hermetic: bool) -> Env {
    let a = tempfile::tempdir().unwrap();
    let cwd = std::fs::canonicalize(a.path()).unwrap();
    let other = cwd.join("sub");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(cwd.join("x.txt"), "x").unwrap();
    let data = tempfile::tempdir().unwrap();
    let zd = tempfile::tempdir().unwrap();
    std::fs::write(
        zd.path().join(".zshrc"),
        "PROMPT='%1~ %# '\nRPROMPT='ok | %D{%H:%M:%S}'\n",
    )
    .unwrap();
    let s = Session::spawn(SpawnOptions {
        shell: PathBuf::from("/bin/zsh"),
        cwd: cwd.clone(),
        size: GridSize { cols, rows: ROWS },
        cell_px: (8, 16),
        scrollback: 1000,
        shim_root: data.path().to_path_buf(),
        theme: Theme::dark(),
        waker: Arc::new(|| {}),
        spool: Some(data.path().join("spool.txt")),
        zdotdir: hermetic.then(|| zd.path().to_path_buf()),
        remote: None,
    })
    .unwrap();
    let mut env = Env {
        s,
        cwd,
        other,
        _dirs: vec![a, data, zd],
    };
    assert!(
        wait(&mut env.s, Duration::from_secs(20), |s| s.is_idle()),
        "no prompt"
    );
    pump(&mut env.s, 300);
    env
}

fn run(env: &mut Env, cmd: &str) {
    env.s.write(cmd.as_bytes());
    env.s.write(b"\r");
    env.s.user_submitted();
    assert!(
        wait(&mut env.s, Duration::from_secs(5), |s| s.is_idle()),
        "command hung: {cmd}"
    );
    pump(&mut env.s, 150);
}

fn resize(env: &mut Env, cols: u16) {
    env.s.resize(GridSize { cols, rows: ROWS }, (8, 16));
    pump(&mut env.s, 400);
}

/// `HH:MM:SS` (or a fragment like `16:5`) somewhere in `l`.
fn has_time_bits(l: &str) -> bool {
    let b: Vec<char> = l.chars().collect();
    b.windows(3)
        .any(|w| w[0].is_ascii_digit() && w[1] == ':' && w[2].is_ascii_digit())
}

/// What the user sees: every right prompt (history and current) ends at
/// the column where zsh puts the CURRENT one at this width (cols-1-indent,
/// minus any trailing blank the theme adds), and none is split or left at
/// an old column.
fn assert_right_aligned(env: &Env, label: &str) {
    let sh = env.s.lock();
    let cols = sh.term.grid().columns();
    let vis = sh.visible_text();
    let dump = || {
        format!(
            "{label} ({cols} cols):\n{}",
            vis.lines().map(|l| format!("|{l}|\n")).collect::<String>()
        )
    };
    let mut ends = Vec::new();
    for l in vis.lines() {
        let t = l.trim_end();
        let stray_ok = t == "ok" || t.ends_with(" ok") || t.contains("ok |") && !has_time_bits(t);
        assert!(!stray_ok, "RPROMPT fragment in {}", dump());
        if has_time_bits(t) {
            assert!(
                t.contains("ok | "),
                "time without its RPROMPT (fragment) in {}",
                dump()
            );
            let tail = &t[t.rfind("ok | ").unwrap()..];
            assert_eq!(
                tail.chars().count(),
                "ok | 00:00:00".len(),
                "split RPROMPT in {}",
                dump()
            );
            ends.push(t.chars().count());
        }
    }
    assert!(
        ends.len() >= 2,
        "expected history + current RPROMPTs in {}",
        dump()
    );
    // The last one is the live prompt, drawn by zsh itself.
    let zsh_end = *ends.last().unwrap();
    assert!(
        zsh_end + 2 >= cols - 1 && zsh_end < cols,
        "zsh's own RPROMPT not at the edge in {}",
        dump()
    );
    assert!(
        ends.iter().all(|&e| e == zsh_end),
        "RPROMPTs at different columns {ends:?} in {}",
        dump()
    );
}

fn scenario(hermetic: bool) {
    if !Path::new("/bin/zsh").exists() {
        return;
    }
    let mut env = spawn(120, hermetic);
    for cmd in ["true", "echo hi", "true"] {
        run(&mut env, cmd);
    }
    assert_right_aligned(&env, "start");
    for cols in [70u16, 140, 97, 61, 120] {
        resize(&mut env, cols);
        assert_right_aligned(&env, "after resize");
    }
    // A burst of tiny changes (splitter drag).
    for cols in [120u16, 119, 117, 120, 118, 121, 120] {
        env.s.resize(GridSize { cols, rows: ROWS }, (8, 16));
        pump(&mut env.s, 12);
    }
    pump(&mut env.s, 500);
    assert_right_aligned(&env, "after burst");

    // cd follow, then a GUI action echo.
    let other = env.other.clone();
    env.s.pane_changed(Some(&other));
    let canon = std::fs::canonicalize(&other).unwrap();
    assert!(wait(&mut env.s, Duration::from_secs(5), |s| {
        s.follow
            .shell_cwd
            .as_ref()
            .is_some_and(|d| d.canon == canon)
            && s.is_idle()
    }));
    pump(&mut env.s, 300);
    assert_right_aligned(&env, "after cd follow");
    let mut a = FileAction::new(
        1,
        PaneId::Left,
        FileActionKind::Copy,
        vec![VfsPath::new(&env.cwd.join("x.txt").to_string_lossy())],
    );
    a.destination = Some(VfsPath::new(&other.to_string_lossy()));
    a.phase = FileActionPhase::Finished {
        ok: true,
        files: 1,
        bytes: 1,
        errors: vec![],
        cancelled: false,
        elapsed_ms: 3,
    };
    env.s.push_action(a);
    assert!(wait(&mut env.s, Duration::from_secs(5), |s| s
        .echo_waiting()
        == 0));
    pump(&mut env.s, 600);
    assert_right_aligned(&env, "after echo");
    resize(&mut env, 80);
    assert_right_aligned(&env, "after echo + resize");
    env.s.shutdown(Duration::from_millis(300));
}

/// The PTY follows only after the size is stable: a burst of sub-cell /
/// splitter changes does not SIGWINCH the shell on every frame.
#[test]
fn resize_is_debounced() {
    use manta_plugin_terminal::session::RESIZE_DEBOUNCE;
    if !Path::new("/bin/zsh").exists() {
        return;
    }
    let mut env = spawn(100, true);
    let px = (8, 16);
    for cols in [99u16, 98, 101, 97] {
        assert!(env
            .s
            .request_resize(GridSize { cols, rows: ROWS }, px)
            .is_some());
        std::thread::sleep(RESIZE_DEBOUNCE / 4);
        assert!(env
            .s
            .request_resize(GridSize { cols, rows: ROWS }, px)
            .is_some());
        assert_eq!(env.s.size().cols, 100, "resized before the size was stable");
    }
    // Same size as now: nothing pending.
    assert!(env
        .s
        .request_resize(
            GridSize {
                cols: 100,
                rows: ROWS
            },
            px
        )
        .is_none());
    let target = GridSize {
        cols: 90,
        rows: ROWS,
    };
    let t = Instant::now();
    while env.s.request_resize(target, px).is_some() {
        assert!(t.elapsed() < RESIZE_DEBOUNCE * 3);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(t.elapsed() >= RESIZE_DEBOUNCE);
    assert_eq!(env.s.size(), target);
    env.s.shutdown(Duration::from_millis(300));
}

#[test]
fn rprompt_right_aligned_hermetic() {
    scenario(true);
}

/// The user's own config (p10k etc.); skipped if it has no RPROMPT like
/// "ok | time" (status + time segments).
#[test]
fn rprompt_right_aligned_user_config() {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let p10k = std::fs::read_to_string(home.join(".p10k.zsh")).unwrap_or_default();
    if !(p10k.contains("status") && p10k.contains("time")) {
        eprintln!("skipping: no p10k status/time right prompt");
        return;
    }
    scenario(false);
}
