//! Real shells: GUI file actions are printed through the shell above the
//! prompt, keep the user's half-typed input, and wait while a program runs.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::VfsPath;
use manta_plugin::{FileAction, FileActionKind, FileActionPhase, PaneId};
use manta_plugin_terminal::palette::Theme;
use manta_plugin_terminal::session::{GridSize, Session, SpawnOptions};

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

struct Env {
    _dirs: Vec<tempfile::TempDir>,
    cwd: PathBuf,
    spool: PathBuf,
}

fn spawn(shell: &str) -> Option<(Session, Env)> {
    if !Path::new(shell).exists() {
        eprintln!("skipping: {shell} not installed");
        return None;
    }
    let a = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let cwd = std::fs::canonicalize(a.path()).unwrap();
    std::fs::create_dir(cwd.join("dest")).unwrap();
    std::fs::write(cwd.join("x.txt"), "hello").unwrap();
    let spool = data.path().join("spool").join("t.txt");
    let s = Session::spawn(SpawnOptions {
        shell: PathBuf::from(shell),
        cwd: cwd.clone(),
        size: GridSize {
            cols: 120,
            rows: 30,
        },
        cell_px: (8, 16),
        scrollback: 1000,
        shim_root: data.path().join("shim"),
        theme: Theme::dark(),
        waker: Arc::new(|| {}),
        spool: Some(spool.clone()),
        zdotdir: None,
        remote: None,
    })
    .expect("spawn");
    Some((
        s,
        Env {
            _dirs: vec![a, data],
            cwd,
            spool,
        },
    ))
}

fn copy_action(env: &Env, id: u64, finished: bool) -> FileAction {
    let mut a = FileAction::new(
        id,
        PaneId::Left,
        FileActionKind::Copy,
        vec![VfsPath::new(&env.cwd.join("x.txt").to_string_lossy())],
    );
    a.destination = Some(VfsPath::new(&env.cwd.join("dest").to_string_lossy()));
    if finished {
        a.phase = FileActionPhase::Finished {
            ok: true,
            files: 1,
            bytes: 5,
            errors: vec![],
            cancelled: false,
            elapsed_ms: 12,
        };
    }
    a
}

fn wait_ready(s: &mut Session, shell: &str) {
    assert!(
        // A slow shell start (e.g. oh-my-zsh under parallel test load) can
        // outlast the integration grace period; the first prompt then
        // upgrades it, so wait for the final answer rather than the first.
        wait_until(s, Duration::from_secs(20), |s| s.is_idle()
            && s.can_echo() == Some(true)),
        "{shell}: no idle prompt. Screen:\n{}",
        s.lock().screen_text()
    );
    assert_eq!(
        s.can_echo(),
        Some(true),
        "{shell}: shim did not announce the echo key"
    );
}

fn line_of(screen: &str, needle: &str) -> Option<usize> {
    screen.lines().position(|l| l.contains(needle))
}

fn rline_of(screen: &str, needle: &str) -> Option<usize> {
    let lines: Vec<&str> = screen.lines().collect();
    lines.iter().rposition(|l| l.contains(needle))
}

const CMD: &str = "→ cp -R 'x.txt' 'dest/'";

fn prints_above_prompt_and_keeps_input(shell: &str) {
    let Some((mut s, env)) = spawn(shell) else {
        return;
    };
    wait_ready(&mut s, shell);

    // Half-typed input.
    s.write(b"echo kee");
    assert!(wait_until(&mut s, Duration::from_secs(5), |s| s
        .lock()
        .screen_text()
        .contains("echo kee")));

    s.push_action(copy_action(&env, 1, false));
    s.push_action(copy_action(&env, 1, true));
    let ok = wait_until(&mut s, Duration::from_secs(5), |s| {
        let t = s.lock().screen_text();
        t.contains(CMD)
            && t.contains("✓ 1 file · 5 B · 12 ms")
            && rline_of(&t, "echo kee") > line_of(&t, CMD)
    });
    let screen = s.lock().screen_text();
    assert!(
        ok,
        "{shell}: action not printed above the prompt. Screen:\n{screen}"
    );
    let cmd_line = line_of(&screen, CMD).unwrap();
    let result_line = line_of(&screen, "✓ 1 file").unwrap();
    let input_line = rline_of(&screen, "echo kee").unwrap();
    assert_eq!(
        result_line,
        cmd_line + 1,
        "{shell}: result right below the command:\n{screen}"
    );
    assert!(
        input_line > result_line,
        "{shell}: prompt with input below the echo:\n{screen}"
    );
    assert!(
        wait_until(&mut s, Duration::from_secs(2), |_| !env.spool.exists()),
        "{shell}: spool file not removed"
    );
    // Nothing was executed: dest/ is still empty.
    assert!(std::fs::read_dir(env.cwd.join("dest"))
        .unwrap()
        .next()
        .is_none());

    // The parked input is intact: finish and run it.
    s.write(b"p_me\r");
    let ok = wait_until(&mut s, Duration::from_secs(5), |s| {
        s.lock()
            .screen_text()
            .lines()
            .any(|l| l.trim() == "keep_me")
    });
    assert!(
        ok,
        "{shell}: typed input was lost. Screen:\n{}",
        s.lock().screen_text()
    );

    // A later, separate result is labelled.
    s.push_action(copy_action(&env, 2, false));
    assert!(wait_until(&mut s, Duration::from_secs(5), |s| s
        .lock()
        .screen_text()
        .matches(CMD)
        .count()
        == 2));
    s.write(b"true\r");
    std::thread::sleep(Duration::from_millis(300));
    s.push_action(copy_action(&env, 2, true));
    let ok = wait_until(&mut s, Duration::from_secs(5), |s| {
        s.lock().screen_text().contains("✓ copy “x.txt” · 1 file")
    });
    assert!(
        ok,
        "{shell}: labelled result missing. Screen:\n{}",
        s.lock().screen_text()
    );
    s.shutdown(Duration::from_millis(500));
}

fn deferred_while_busy(shell: &str) {
    let Some((mut s, env)) = spawn(shell) else {
        return;
    };
    wait_ready(&mut s, shell);
    s.write(b"sleep 2\r");
    s.user_submitted();
    assert!(wait_until(&mut s, Duration::from_secs(3), |s| s.is_busy()));
    let t0 = Instant::now();
    s.push_action(copy_action(&env, 1, false));
    s.push_action(copy_action(&env, 1, true));
    // While sleep runs nothing is printed or written.
    while t0.elapsed() < Duration::from_millis(1000) {
        s.tick(true);
        assert!(
            !s.lock().screen_text().contains('→'),
            "{shell}: printed while busy"
        );
        assert!(!env.spool.exists(), "{shell}: spool written while busy");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(s.echo_waiting(), 1);
    let ok = wait_until(&mut s, Duration::from_secs(6), |s| {
        s.lock().screen_text().contains(CMD)
    });
    assert!(
        ok,
        "{shell}: deferred echo not printed. Screen:\n{}",
        s.lock().screen_text()
    );
    assert!(
        t0.elapsed() >= Duration::from_millis(1500),
        "{shell}: printed before sleep ended"
    );
    s.shutdown(Duration::from_millis(500));
}

/// The cd follow and the echo keep the order they happened in.
fn echo_and_cd_keep_order(shell: &str) {
    let Some((mut s, env)) = spawn(shell) else {
        return;
    };
    wait_ready(&mut s, shell);
    s.write(b"sleep 1\r");
    s.user_submitted();
    assert!(wait_until(&mut s, Duration::from_secs(3), |s| s.is_busy()));
    s.push_action(copy_action(&env, 1, true));
    s.pane_changed(Some(&env.cwd.join("dest")));
    let dest = env.cwd.join("dest");
    let ok = wait_until(&mut s, Duration::from_secs(6), |s| {
        s.follow.shell_cwd.as_ref().map(|d| d.canon.clone()) == Some(dest.clone())
    });
    assert!(
        ok,
        "{shell}: cd not applied. Screen:\n{}",
        s.lock().screen_text()
    );
    let screen = s.lock().screen_text();
    // The result was printed while the shell was still in the old folder.
    let echo =
        line_of(&screen, "✓ copy “x.txt”").unwrap_or_else(|| panic!("{shell}: no echo:\n{screen}"));
    assert!(!s.follow.has_pending());
    // And the prompt after the cd comes below it.
    let last_prompt = screen
        .lines()
        .collect::<Vec<_>>()
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .unwrap();
    assert!(last_prompt > echo, "{shell}:\n{screen}");
    s.shutdown(Duration::from_millis(500));
}

fn modern_bash() -> &'static str {
    ["/opt/homebrew/bin/bash", "/usr/local/bin/bash", "/bin/bash"]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .unwrap_or("/bin/bash")
}

#[test]
fn zsh_prints_above_prompt_and_keeps_input() {
    prints_above_prompt_and_keeps_input("/bin/zsh");
}

#[test]
fn bash_prints_above_prompt_and_keeps_input() {
    prints_above_prompt_and_keeps_input(modern_bash());
}

#[test]
fn bash3_prints_above_prompt_and_keeps_input() {
    if modern_bash() != "/bin/bash" {
        prints_above_prompt_and_keeps_input("/bin/bash");
    }
}

#[test]
fn zsh_defers_echo_while_program_runs() {
    deferred_while_busy("/bin/zsh");
}

#[test]
fn bash_defers_echo_while_program_runs() {
    deferred_while_busy(modern_bash());
}

#[test]
fn zsh_echo_and_cd_keep_order() {
    echo_and_cd_keep_order("/bin/zsh");
}

#[test]
fn bash_echo_and_cd_keep_order() {
    echo_and_cd_keep_order(modern_bash());
}

/// zsh replaces the prompt with the echo: with a two-line prompt both prompt
/// lines go, earlier output stays.
#[test]
fn zsh_echo_replaces_multiline_prompt_only() {
    let Some((mut s, env)) = spawn("/bin/zsh") else {
        return;
    };
    wait_ready(&mut s, "zsh");
    // Plain prompt: no prompt framework hooks (they rewrite PROMPT).
    s.write(b"precmd_functions=(_manta_precmd); zle -D zle-line-init zle-line-finish 2>/dev/null; clear; PROMPT=$'[top line]\\n%# '; RPROMPT=; echo earlier-output\r");
    s.user_submitted();
    assert!(
        wait_until(&mut s, Duration::from_secs(5), |s| {
            let t = s.lock().screen_text();
            s.is_idle() && t.contains("[top line]") && t.contains("earlier-output")
        }),
        "{}",
        s.lock().screen_text()
    );
    s.write(b"echo kee");
    assert!(wait_until(&mut s, Duration::from_secs(5), |s| s
        .lock()
        .screen_text()
        .contains("% echo kee")));
    s.push_action(copy_action(&env, 1, true));
    let ok = wait_until(&mut s, Duration::from_secs(5), |s| {
        s.lock().screen_text().contains("✓ copy")
    });
    let screen = s.lock().screen_text();
    assert!(ok, "no echo:\n{screen}");
    let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    let i = lines
        .iter()
        .position(|l| l.contains("earlier-output"))
        .unwrap();
    assert_eq!(
        &lines[i..i + 4],
        &[
            "earlier-output",
            "  ✓ copy “x.txt” · 1 file · 5 B · 12 ms",
            "[top line]",
            "% echo kee"
        ],
        "{screen}"
    );
    s.shutdown(Duration::from_millis(500));
}
