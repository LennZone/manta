//! [`ExternalPlugin`]: runs a user-installed plugin as a child process and
//! adapts it to the [`Plugin`] trait.
//!
//! Threads per running plugin (none of them ever block the UI thread):
//! - a **supervisor** that spawns the process, waits for it to exit,
//!   restarts it after a crash (at most [`MAX_RESTARTS`] per minute) and,
//!   when the plugin is switched off, kills it if it hasn't exited
//!   [`SHUTDOWN_GRACE`] after `shutdown`;
//! - a **writer** fed by a channel, so sending an event never waits on the
//!   plugin's stdin;
//! - a **reader** that parses stdout lines and forwards them to the host;
//! - a **stderr reader** that appends to `<data_dir>/plugin.log` and keeps
//!   the last [`LOG_TAIL`] lines for Settings.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::manifest::Manifest;
use crate::protocol::{self, Block, HostMessage, LogLevel, Parsed, PluginMessage};
use crate::{
    CommandSpec, HostRequest, PaneId, PaneState, PanelSpec, Plugin, PluginEvent, PluginHandle,
    PluginInfo, PluginStatus, ToastLevel,
};

/// Restarts allowed within [`RESTART_WINDOW`] before the plugin is left
/// in the error state.
pub const MAX_RESTARTS: usize = 3;
pub const RESTART_WINDOW: Duration = Duration::from_secs(60);
/// Time a plugin gets to exit after `shutdown` before it is killed.
pub const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);
/// Lines of log kept in memory for Settings.
pub const LOG_TAIL: usize = 50;
const LOG_FILE: &str = "plugin.log";
/// The log file is truncated at start when it grew beyond this.
const LOG_FILE_MAX: u64 = 512 * 1024;
/// Events queued while the process is (re)starting.
const MAX_PENDING: usize = 256;

type Content = HashMap<(String, Option<PaneId>), Arc<Vec<Block>>>;
/// Panel content keyed by (panel, pane), for [`ExternalPlugin::inject_state`].
pub type InjectedContent = Vec<((String, Option<PaneId>), Vec<Block>)>;

/// State shared between the UI thread and the plugin's threads.
struct Shared {
    /// Bumped on every enable; threads of an older run don't touch state.
    run: u64,
    status: PluginStatus,
    log_tail: VecDeque<String>,
    log_file: Option<File>,
    /// Registered at runtime; `None` = as declared in the manifest.
    commands: Option<Vec<CommandSpec>>,
    panels: Option<Vec<PanelSpec>>,
    /// Modal sizes from `register_panels` (with `panels`).
    panel_sizes: HashMap<String, [f32; 2]>,
    content: Content,
    /// Last known state of both panes (for `init` after a restart).
    panes: std::collections::BTreeMap<PaneId, PaneState>,
    /// Channel to the writer thread while the process runs.
    tx: Option<Sender<String>>,
    /// Lines sent while no process is running yet.
    pending: VecDeque<String>,
    /// Processes (of any run) that haven't exited yet.
    live: usize,
}

impl Shared {
    fn log(&mut self, line: impl Into<String>) {
        let line = line.into();
        if let Some(f) = &mut self.log_file {
            let _ = writeln!(f, "{line}");
        }
        self.log_tail.push_back(line);
        while self.log_tail.len() > LOG_TAIL {
            self.log_tail.pop_front();
        }
    }

    fn send(&mut self, line: String) {
        if let Some(tx) = &self.tx {
            if tx.send(line).is_ok() {
                return;
            }
            return; // writer gone: the process is exiting
        }
        if self.pending.len() >= MAX_PENDING {
            self.pending.pop_front();
        }
        self.pending.push_back(line);
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    // A panic on a plugin thread must not poison the UI.
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// An installed external plugin (see the module docs).
pub struct ExternalPlugin {
    manifest: Manifest,
    dir: PathBuf,
    shared: Arc<Mutex<Shared>>,
    /// Stop flag of the current run.
    stop: Option<Arc<AtomicBool>>,
}

impl ExternalPlugin {
    /// `dir` is the plugin folder containing `plugin.toml`.
    pub fn new(manifest: Manifest, dir: PathBuf) -> Self {
        ExternalPlugin {
            manifest,
            dir,
            shared: Arc::new(Mutex::new(Shared {
                run: 0,
                status: PluginStatus::Stopped,
                log_tail: VecDeque::new(),
                log_file: None,
                commands: None,
                panels: None,
                panel_sizes: HashMap::new(),
                content: HashMap::new(),
                panes: Default::default(),
                tx: None,
                pending: VecDeque::new(),
                live: 0,
            })),
            stop: None,
        }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn status(&self) -> PluginStatus {
        lock(&self.shared).status.clone()
    }

    /// The last lines of stderr and `log` messages.
    pub fn log_tail(&self) -> Vec<String> {
        lock(&self.shared).log_tail.iter().cloned().collect()
    }

    /// Blocks of a panel instance (`pane` for `pane_bottom` panels); falls
    /// back to content set for all panes.
    pub fn panel_blocks(&self, panel: &str, pane: Option<PaneId>) -> Option<Arc<Vec<Block>>> {
        let s = lock(&self.shared);
        s.content
            .get(&(panel.to_string(), pane))
            .or_else(|| s.content.get(&(panel.to_string(), None)))
            .cloned()
    }

    /// Forward a click on a row or button with an `action`.
    pub fn panel_action(&mut self, panel: &str, action: &str, pane: &PaneState) {
        let line = HostMessage::PanelAction {
            panel: panel.into(),
            action: action.into(),
            pane: pane.clone(),
        }
        .to_line();
        lock(&self.shared).send(line);
    }

    /// Test seam: put the plugin in a state without running anything.
    #[doc(hidden)]
    pub fn inject_state(&mut self, status: PluginStatus, log: &[&str], content: InjectedContent) {
        let mut s = lock(&self.shared);
        s.status = status;
        s.log_tail = log.iter().map(|l| l.to_string()).collect();
        for (k, v) in content {
            s.content.insert(k, Arc::new(v));
        }
    }

    /// Wait (on the calling thread) until the process has exited, at most
    /// `timeout`. Only for app exit and tests.
    pub fn wait_stopped(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            if lock(&self.shared).live == 0 {
                return true;
            }
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Plugin for ExternalPlugin {
    fn info(&self) -> PluginInfo {
        self.manifest.info()
    }

    fn enable(&mut self, handle: PluginHandle) {
        if self.stop.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Some(stop.clone());
        let run = {
            let mut s = lock(&self.shared);
            s.run += 1;
            s.status = PluginStatus::Starting;
            s.commands = None;
            s.panels = None;
            s.content.clear();
            s.pending.clear();
            s.run
        };
        let launch = Launch {
            manifest: self.manifest.clone(),
            dir: self.dir.clone(),
            data_dir: handle.data_dir().clone(),
            locale: crate::i18n::thread_lang().map(|l| l.code().to_string()),
        };
        let shared = self.shared.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("plugin-{}", self.manifest.id))
            .spawn(move || supervise(launch, shared, stop, handle, run));
        if let Err(e) = spawned {
            lock(&self.shared).status =
                PluginStatus::Error(format!("couldn't start a thread: {e}"));
        }
    }

    fn disable(&mut self) {
        let Some(stop) = self.stop.take() else {
            return;
        };
        stop.store(true, Ordering::SeqCst);
        let mut s = lock(&self.shared);
        s.run += 1; // the old run's threads stop touching shared state
        if let Some(tx) = s.tx.take() {
            let _ = tx.send(HostMessage::Shutdown.to_line());
            // Dropping `tx` lets the writer close stdin after `shutdown`.
        }
        s.pending.clear();
        s.content.clear();
        s.commands = None;
        s.panels = None;
        if !matches!(s.status, PluginStatus::Error(_)) {
            s.status = PluginStatus::Stopped;
        }
    }

    fn on_event(&mut self, event: &PluginEvent) {
        let mut s = lock(&self.shared);
        let msg = match event {
            // Only for the built-in Terminal.
            PluginEvent::RunInTerminal { .. } => return,
            PluginEvent::DirectoryChanged(p) => {
                s.panes.insert(p.pane, p.clone());
                HostMessage::DirectoryChanged {
                    pane_state: p.clone(),
                }
            }
            PluginEvent::ListingRefreshed(p) => {
                s.panes.insert(p.pane, p.clone());
                HostMessage::ListingRefreshed {
                    pane_state: p.clone(),
                }
            }
            PluginEvent::SelectionChanged(p) => {
                s.panes.insert(p.pane, p.clone());
                HostMessage::SelectionChanged {
                    pane_state: p.clone(),
                }
            }
            PluginEvent::ActivePaneChanged(id) => {
                for p in s.panes.values_mut() {
                    p.active = p.pane == *id;
                }
                HostMessage::ActivePaneChanged { pane: *id }
            }
            PluginEvent::PaneOpened(id) => HostMessage::PaneOpened { pane: *id },
            PluginEvent::PaneClosed(id) => {
                s.panes.remove(id);
                s.content.retain(|(_, p), _| *p != Some(*id));
                HostMessage::PaneClosed { pane: *id }
            }
            PluginEvent::FileAction(a) => HostMessage::FileAction { action: a.clone() },
            PluginEvent::LocaleChanged(l) => HostMessage::LocaleChanged { locale: l.clone() },
            PluginEvent::RenameResult(r) => HostMessage::RenameResult {
                pane: r.pane,
                dir: r.dir.clone(),
                ok: r.ok.clone(),
                failed: r.failed.clone(),
            },
            PluginEvent::DirListing(l) => HostMessage::DirListing {
                pane: l.pane,
                dir: l.dir.clone(),
                entries: l.entries.clone(),
                error: l.error.clone(),
            },
            PluginEvent::FileContent(c) => {
                use base64::Engine as _;
                HostMessage::FileContent {
                    pane: c.pane,
                    path: c.path.clone(),
                    data: base64::engine::general_purpose::STANDARD.encode(&c.bytes),
                    size: c.size,
                    truncated: c.truncated,
                    modified: c.modified,
                    error: c.error.clone(),
                }
            }
        };
        s.send(msg.to_line());
    }

    fn commands(&self) -> Vec<CommandSpec> {
        lock(&self.shared).commands.clone().unwrap_or_else(|| {
            self.manifest
                .commands
                .iter()
                .cloned()
                .map(Into::into)
                .collect()
        })
    }

    fn run_command(&mut self, id: &str, pane: &PaneState) {
        let line = HostMessage::RunCommand {
            id: id.into(),
            pane: pane.clone(),
        }
        .to_line();
        lock(&self.shared).send(line);
    }

    fn panel_size(&self, panel: &str) -> Option<[f32; 2]> {
        let s = lock(&self.shared);
        if s.panels.is_some() {
            return s.panel_sizes.get(panel).copied();
        }
        self.manifest
            .panels
            .iter()
            .find(|p| p.id == panel)
            .and_then(|p| p.size)
    }

    fn panels(&self) -> Vec<PanelSpec> {
        lock(&self.shared).panels.clone().unwrap_or_else(|| {
            self.manifest
                .panels
                .iter()
                .cloned()
                .map(Into::into)
                .collect()
        })
    }
}

impl Drop for ExternalPlugin {
    fn drop(&mut self) {
        self.disable();
    }
}

// ------------------------------------------------------------------ process

struct Launch {
    manifest: Manifest,
    dir: PathBuf,
    data_dir: PathBuf,
    /// The starting thread's language override (tests and renders); the
    /// app's language otherwise.
    locale: Option<String>,
}

/// Directories GUI apps on macOS don't have on their PATH but plugin
/// interpreters often live in.
const EXTRA_PATH: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];

fn child_path_var() -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for d in EXTRA_PATH {
        let d = PathBuf::from(d);
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

fn spawn(l: &Launch) -> std::io::Result<Child> {
    let argv = &l.manifest.command;
    let program = &argv[0];
    // "./run.sh" or "bin/plugin": relative to the plugin folder.
    let program: PathBuf = if program.contains('/') && !Path::new(program).is_absolute() {
        l.dir.join(program)
    } else {
        PathBuf::from(program)
    };
    Command::new(program)
        .args(&argv[1..])
        .current_dir(&l.dir)
        .env("PATH", child_path_var())
        .env("MANTA_PLUGIN_ID", &l.manifest.id)
        .env("MANTA_PLUGIN_DATA_DIR", &l.data_dir)
        .env("MANTA_API_VERSION", protocol::PROTOCOL_VERSION.to_string())
        // Python buffers stdout when it isn't a terminal; plugins written
        // in it would otherwise look frozen.
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

fn open_log(data_dir: &Path) -> Option<File> {
    std::fs::create_dir_all(data_dir).ok()?;
    let path = data_dir.join(LOG_FILE);
    let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_FILE_MAX);
    std::fs::OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(path)
        .ok()
}

fn describe_exit(st: Option<ExitStatus>) -> String {
    match st {
        Some(st) => match st.code() {
            Some(c) => format!("exited with status {c}"),
            None => {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    if let Some(sig) = st.signal() {
                        return format!("was killed by signal {sig}");
                    }
                }
                "exited".into()
            }
        },
        None => "stopped (exit status unknown)".into(),
    }
}

/// Sleep up to `d`, returning early (true) once `stop` is set.
fn sleep_unless(stop: &AtomicBool, d: Duration) -> bool {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    stop.load(Ordering::SeqCst)
}

fn supervise(
    l: Launch,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    handle: PluginHandle,
    run: u64,
) {
    let name = l.manifest.name.clone();
    let program = l.manifest.command[0].clone();
    // Update shared state only while this run is current.
    let with = |f: &mut dyn FnMut(&mut Shared)| {
        let mut s = lock(&shared);
        if s.run == run {
            f(&mut s);
        }
    };
    with(&mut |s| {
        s.log_file = open_log(&l.data_dir);
    });
    let mut crashes: VecDeque<Instant> = VecDeque::new();

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        with(&mut |s| s.status = PluginStatus::Starting);
        let mut child = match spawn(&l) {
            Ok(c) => c,
            Err(e) => {
                let msg = if e.kind() == std::io::ErrorKind::NotFound {
                    format!("couldn't start “{program}”: program not found")
                } else {
                    format!("couldn't start “{program}”: {e}")
                };
                with(&mut |s| {
                    s.log(format!("[manta] {msg}"));
                    s.status = PluginStatus::Error(msg.clone());
                });
                handle.send(HostRequest::Toast {
                    level: ToastLevel::Error,
                    text: format!("Plugin “{name}” {msg}"),
                });
                return;
            }
        };
        let pid = child.id();
        lock(&shared).live += 1;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let (tx, rx) = mpsc::channel::<String>();
        if let Some(mut stdin) = stdin {
            std::thread::spawn(move || {
                for line in rx {
                    if stdin.write_all(line.as_bytes()).is_err() || stdin.flush().is_err() {
                        break;
                    }
                }
                // Dropping stdin sends EOF.
            });
        }
        if let Some(out) = stdout {
            let (shared, handle) = (shared.clone(), handle.clone());
            std::thread::spawn(move || read_stdout(out, shared, handle, run));
        }
        if let Some(err) = stderr {
            let shared = shared.clone();
            std::thread::spawn(move || read_stderr(err, shared));
        }

        // `init` first, then whatever queued up while starting.
        {
            let mut s = lock(&shared);
            if s.run == run && !stop.load(Ordering::SeqCst) {
                let init = HostMessage::Init {
                    api_version: protocol::PROTOCOL_VERSION,
                    plugin_id: l.manifest.id.clone(),
                    data_dir: l.data_dir.clone(),
                    panes: s.panes.values().cloned().collect(),
                    locale: l.locale.clone().unwrap_or_else(crate::locale),
                };
                let _ = tx.send(init.to_line());
                for line in s.pending.drain(..) {
                    let _ = tx.send(line);
                }
                s.tx = Some(tx);
                s.status = PluginStatus::Running;
                s.log(format!("[manta] started (pid {pid})"));
            } else {
                let _ = tx.send(HostMessage::Shutdown.to_line());
                drop(tx);
            }
        }
        handle.request_repaint();

        // Wait for exit; after `stop`, give it SHUTDOWN_GRACE, then kill.
        let mut deadline: Option<Instant> = None;
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Some(st),
                Ok(None) => {}
                Err(_) => break None,
            }
            if stop.load(Ordering::SeqCst) {
                let d = *deadline.get_or_insert_with(|| Instant::now() + SHUTDOWN_GRACE);
                if Instant::now() >= d {
                    let _ = child.kill();
                    break child.wait().ok();
                }
            }
            std::thread::sleep(Duration::from_millis(15));
        };
        let stopping = stop.load(Ordering::SeqCst);
        lock(&shared).live -= 1;
        with(&mut |s| {
            s.tx = None;
        });
        if stopping {
            let mut s = lock(&shared);
            // Logged even if a newer run started (it's the same log).
            s.log(format!("[manta] stopped ({})", describe_exit(status)));
            break;
        }

        // Crashed (or quit on its own).
        let what = describe_exit(status);
        let now = Instant::now();
        crashes.push_back(now);
        while crashes
            .front()
            .is_some_and(|t| now.duration_since(*t) > RESTART_WINDOW)
        {
            crashes.pop_front();
        }
        let give_up = crashes.len() > MAX_RESTARTS;
        let msg = if give_up {
            format!("{what}; not restarted after {MAX_RESTARTS} restarts within a minute")
        } else {
            format!("{what}; restarting…")
        };
        with(&mut |s| {
            s.log(format!("[manta] {msg}"));
            s.status = PluginStatus::Error(msg.clone());
            s.content.clear();
        });
        handle.send(HostRequest::ContributionsChanged);
        if give_up {
            handle.send(HostRequest::Toast {
                level: ToastLevel::Error,
                text: format!("Plugin “{name}” {what} and was stopped"),
            });
            return;
        }
        if sleep_unless(&stop, Duration::from_millis(500 * crashes.len() as u64)) {
            break;
        }
    }
}

/// Outcome of [`read_line_capped`].
enum Line {
    Ok,
    TooLong,
    Eof,
}

/// Read one `\n`-terminated line into `buf` without ever holding more
/// than `max` bytes; longer lines are skipped.
fn read_line_capped<R: BufRead>(r: &mut R, buf: &mut Vec<u8>, max: usize) -> std::io::Result<Line> {
    buf.clear();
    let mut too_long = false;
    loop {
        let (used, done) = {
            let avail = match r.fill_buf() {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if avail.is_empty() {
                return Ok(if too_long {
                    Line::TooLong
                } else if buf.is_empty() {
                    Line::Eof
                } else {
                    Line::Ok
                });
            }
            let nl = avail.iter().position(|&b| b == b'\n');
            let chunk = &avail[..nl.unwrap_or(avail.len())];
            if !too_long {
                if buf.len() + chunk.len() > max {
                    too_long = true;
                    buf.clear();
                } else {
                    buf.extend_from_slice(chunk);
                }
            }
            (chunk.len() + usize::from(nl.is_some()), nl.is_some())
        };
        r.consume(used);
        if done {
            return Ok(if too_long { Line::TooLong } else { Line::Ok });
        }
    }
}

fn read_stdout(out: impl Read, shared: Arc<Mutex<Shared>>, handle: PluginHandle, run: u64) {
    let mut r = BufReader::new(out);
    let mut buf = Vec::new();
    loop {
        match read_line_capped(&mut r, &mut buf, protocol::MAX_LINE_BYTES) {
            Ok(Line::Eof) | Err(_) => break,
            Ok(Line::TooLong) => {
                let mut s = lock(&shared);
                s.log(format!(
                    "[manta] ignored a message longer than {} bytes",
                    protocol::MAX_LINE_BYTES
                ));
            }
            Ok(Line::Ok) => {
                // After `disable`, keep draining the pipe (so the process
                // can exit) but ignore what it says.
                if lock(&shared).run != run {
                    continue;
                }
                let line = String::from_utf8_lossy(&buf);
                handle_line(&line, &shared, &handle);
            }
        }
    }
}

/// Act on one line from the plugin's stdout.
fn handle_line(line: &str, shared: &Mutex<Shared>, handle: &PluginHandle) {
    let msg = match protocol::parse_line(line) {
        Parsed::Message(m) => *m,
        Parsed::Unknown(ty) => {
            lock(shared).log(format!("[manta] ignored unknown message type “{ty}”"));
            return;
        }
        Parsed::Invalid(e) if e.is_empty() => return,
        Parsed::Invalid(e) => {
            let mut preview: String = line.chars().take(120).collect();
            if preview.len() < line.len() {
                preview.push('…');
            }
            lock(shared).log(format!("[manta] {e}: {preview}"));
            return;
        }
    };
    match msg {
        PluginMessage::SetDecorations { pane, dir, entries } => {
            handle.send(HostRequest::SetDecorations {
                pane,
                dir,
                entries: entries.into_iter().collect(),
            });
        }
        PluginMessage::SetSizes { pane, dir, sizes } => {
            handle.send(HostRequest::SetSizes {
                pane,
                dir,
                sizes: sizes.into_iter().collect(),
            });
        }
        PluginMessage::SetChips { pane, dir, chips } => {
            handle.send(HostRequest::SetChips { pane, dir, chips });
        }
        PluginMessage::Toast {
            level,
            text,
            button,
            command,
        } => match (button, command) {
            (Some(button), Some(command)) => handle.send(HostRequest::ActionToast {
                level,
                text,
                button,
                command,
            }),
            _ => handle.send(HostRequest::Toast { level, text }),
        },
        PluginMessage::RenameMany { pane, dir, renames } => {
            handle.send(HostRequest::RenameMany { pane, dir, renames })
        }
        PluginMessage::ListDir { pane, dir } => handle.send(HostRequest::ListDir { pane, dir }),
        PluginMessage::ReadFile {
            pane,
            path,
            max_bytes,
        } => handle.send(HostRequest::ReadFile {
            pane,
            path,
            max_bytes,
        }),
        PluginMessage::RunInTerminal { pane, command, cwd } => {
            handle.send(HostRequest::RunInTerminal { pane, command, cwd })
        }
        PluginMessage::RefreshPane { pane } => handle.send(HostRequest::RefreshPane { pane }),
        PluginMessage::Select { pane, paths } => handle.send(HostRequest::Select { pane, paths }),
        PluginMessage::SetStatusItems { items } => {
            handle.send(HostRequest::SetStatusItems { items })
        }
        PluginMessage::Navigate {
            pane,
            path,
            remote: None,
        } => handle.send(HostRequest::Navigate { pane, path }),
        PluginMessage::Navigate { pane, path, remote } => {
            handle.send(HostRequest::NavigateIn { pane, remote, path })
        }
        PluginMessage::RegisterCommands { commands } => {
            lock(shared).commands = Some(commands.into_iter().map(Into::into).collect());
            handle.send(HostRequest::ContributionsChanged);
        }
        PluginMessage::RegisterPanels { panels } => {
            let mut s = lock(shared);
            s.panel_sizes = panels
                .iter()
                .filter_map(|p| Some((p.id.clone(), p.size?)))
                .collect();
            s.panels = Some(panels.into_iter().map(Into::into).collect());
            drop(s);
            handle.send(HostRequest::ContributionsChanged);
        }
        PluginMessage::SetPanelContent {
            panel,
            pane,
            blocks,
        } => {
            let mut s = lock(shared);
            if pane.is_none() {
                // Content for all panes replaces per-pane content.
                s.content.retain(|(p, _), _| *p != panel);
            }
            s.content.insert((panel, pane), Arc::new(blocks));
            drop(s);
            handle.request_repaint();
        }
        PluginMessage::SetPanelOpen { panel, pane, open } => {
            handle.send(HostRequest::SetPanelOpen { panel, pane, open });
        }
        PluginMessage::TogglePanel { panel, pane } => {
            handle.send(HostRequest::TogglePanel { panel, pane });
        }
        PluginMessage::Log { level, message } => {
            let tag = match level {
                LogLevel::Debug => "debug",
                LogLevel::Info => "info",
                LogLevel::Warn => "warn",
                LogLevel::Error => "error",
            };
            lock(shared).log(format!("[{tag}] {message}"));
            handle.request_repaint();
        }
    }
}

fn read_stderr(err: impl Read, shared: Arc<Mutex<Shared>>) {
    let mut r = BufReader::new(err);
    let mut buf = Vec::new();
    loop {
        match read_line_capped(&mut r, &mut buf, 16 * 1024) {
            Ok(Line::Eof) | Err(_) => break,
            Ok(Line::TooLong) => lock(&shared).log("[stderr] (very long line skipped)"),
            Ok(Line::Ok) => {
                let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                // stderr of an old run still belongs in the log.
                lock(&shared).log(line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_lines() {
        let data = b"short\nthis line is far too long\nok\nend";
        let mut r = BufReader::with_capacity(4, &data[..]);
        let mut buf = Vec::new();
        let mut out = Vec::new();
        loop {
            match read_line_capped(&mut r, &mut buf, 10).unwrap() {
                Line::Eof => break,
                Line::TooLong => out.push("<too long>".to_string()),
                Line::Ok => out.push(String::from_utf8(buf.clone()).unwrap()),
            }
        }
        assert_eq!(out, vec!["short", "<too long>", "ok", "end"]);
    }

    #[test]
    fn exit_descriptions() {
        assert_eq!(describe_exit(None), "stopped (exit status unknown)");
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(
                describe_exit(Some(ExitStatus::from_raw(1 << 8))),
                "exited with status 1"
            );
            assert_eq!(
                describe_exit(Some(ExitStatus::from_raw(9))),
                "was killed by signal 9"
            );
        }
    }
}
