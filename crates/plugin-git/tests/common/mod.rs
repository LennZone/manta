//! Test helpers: temp repos driven by the real `git`, and a fake host that
//! collects what the plugin sends.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use manta_core::{Protocol, VfsPath};
use manta_plugin::{
    Chip, Decoration, HostRequest, PaneId, PaneState, Plugin, PluginEvent, PluginHandle,
};
use manta_plugin_git::GitPlugin;

pub fn git_available() -> bool {
    let ok = Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("git not available, skipping");
    }
    ok
}

/// Run git in `dir` and assert success; returns stdout.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Run git and return whether it succeeded (for expected failures like
/// conflicting merges).
pub fn git_try(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn configure(dir: &Path) {
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["config", "user.email", "test@example.org"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    git(dir, &["config", "core.hooksPath", "/dev/null"]);
}

pub struct Repo {
    pub tmp: tempfile::TempDir,
    pub path: PathBuf,
}

impl Repo {
    pub fn new() -> Repo {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("project");
        std::fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-q", "-b", "main"]);
        configure(&path);
        Repo { tmp, path }
    }

    pub fn write(&self, rel: &str, content: &str) {
        let p = self.path.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, content).unwrap();
    }

    pub fn git(&self, args: &[&str]) -> String {
        git(&self.path, args)
    }

    pub fn commit_all(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }
}

pub fn pane_state(pane: PaneId, dir: Option<&Path>) -> PaneState {
    let path = match dir {
        Some(d) => VfsPath::new(&d.to_string_lossy()),
        None => VfsPath::new("/home/anna"),
    };
    PaneState {
        remote: None,
        pane,
        path,
        local_path: dir.map(Path::to_path_buf),
        protocol: if dir.is_some() {
            Protocol::Local
        } else {
            Protocol::Sftp
        },
        backend_label: if dir.is_some() {
            "Local".into()
        } else {
            "anna@example.org (SFTP)".into()
        },
        selection: Vec::new(),
        active: pane == PaneId::Left,
    }
}

pub fn vfs(dir: &Path) -> VfsPath {
    VfsPath::new(&dir.to_string_lossy())
}

/// Collects the latest decorations / chips per (pane, dir).
pub struct FakeHost {
    pub plugin: GitPlugin,
    rx: Receiver<(String, HostRequest)>,
    pub decorations: HashMap<(PaneId, VfsPath), Vec<(String, Decoration)>>,
    pub chips: HashMap<(PaneId, VfsPath), Vec<Chip>>,
    pub toasts: Vec<String>,
    pub panel_requests: Vec<bool>,
    pub panel_toggles: usize,
    pub decoration_sends: usize,
    _data: tempfile::TempDir,
}

impl FakeHost {
    pub fn new() -> FakeHost {
        let (tx, rx) = mpsc::channel();
        let data = tempfile::tempdir().unwrap();
        let handle = PluginHandle::new(
            "manta.git".into(),
            tx,
            egui::Context::default(),
            data.path().to_path_buf(),
        );
        let mut plugin = GitPlugin::new();
        plugin.enable(handle);
        FakeHost {
            plugin,
            rx,
            decorations: HashMap::new(),
            chips: HashMap::new(),
            toasts: Vec::new(),
            panel_requests: Vec::new(),
            panel_toggles: 0,
            decoration_sends: 0,
            _data: data,
        }
    }

    pub fn show(&mut self, pane: PaneId, dir: Option<&Path>) {
        self.plugin
            .on_event(&PluginEvent::DirectoryChanged(pane_state(pane, dir)));
    }

    pub fn relist(&mut self, pane: PaneId, dir: &Path) {
        self.plugin
            .on_event(&PluginEvent::ListingRefreshed(pane_state(pane, Some(dir))));
    }

    pub fn pump(&mut self) {
        while let Ok((id, req)) = self.rx.try_recv() {
            assert_eq!(id, "manta.git");
            match req {
                HostRequest::SetDecorations { pane, dir, entries } => {
                    self.decoration_sends += 1;
                    self.decorations.insert((pane, dir), entries);
                }
                HostRequest::SetChips { pane, dir, chips } => {
                    self.chips.insert((pane, dir), chips);
                }
                HostRequest::Toast { text, .. } => self.toasts.push(text),
                HostRequest::SetPanelOpen { open, .. } => self.panel_requests.push(open),
                HostRequest::TogglePanel { .. } => self.panel_toggles += 1,
                _ => {}
            }
        }
    }

    /// Pump until `pred` holds or `timeout` passes. Returns whether it held.
    pub fn wait_until(
        &mut self,
        timeout: Duration,
        mut pred: impl FnMut(&FakeHost) -> bool,
    ) -> bool {
        let start = Instant::now();
        loop {
            self.pump();
            if pred(self) {
                return true;
            }
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn deco(&self, pane: PaneId, dir: &Path, name: &str) -> Option<Decoration> {
        self.decorations
            .get(&(pane, vfs(dir)))?
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d.clone())
    }

    pub fn badge(
        &self,
        pane: PaneId,
        dir: &Path,
        name: &str,
    ) -> Option<(String, manta_plugin::Tone)> {
        let b = self.deco(pane, dir, name)?.badge?;
        Some((b.text, b.tone))
    }

    pub fn chip(&self, pane: PaneId, dir: &Path) -> Option<Chip> {
        self.chips.get(&(pane, vfs(dir)))?.first().cloned()
    }
}

impl Drop for FakeHost {
    fn drop(&mut self) {
        self.plugin.disable();
    }
}

/// Generous: starting the FSEvents watchers can take seconds when the test
/// binary lives in a huge `target/debug/deps` (seen with 280k files there).
pub const WAIT: Duration = Duration::from_secs(30);
