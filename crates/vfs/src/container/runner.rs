//! Running the `docker` / `kubectl` CLIs: a small [`Runner`] trait (so the
//! backends can be tested with a mock), the real [`CliRunner`] with
//! timeouts, and the mapping of CLI error texts to friendly [`VfsError`]s.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::VfsError;

/// Timeout for metadata commands (list, stat, mkdir, inspect, ...).
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout for discovery commands (`docker ps`, `kubectl get pods`, ...).
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);

/// Result of a finished command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CmdOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl CmdOutput {
    pub fn ok(stdout: impl Into<Vec<u8>>) -> CmdOutput {
        CmdOutput {
            success: true,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn fail(stderr: impl Into<String>) -> CmdOutput {
        CmdOutput {
            success: false,
            stdout: Vec::new(),
            stderr: stderr.into(),
        }
    }

    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

/// A running, streaming command (`docker cp … -`, `exec -i … cat > f`).
pub trait Proc: Send {
    fn take_stdin(&mut self) -> Option<Box<dyn Write + Send>>;
    fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>>;
    /// Wait for the exit; `(success, stderr)`. Close stdin first.
    fn wait(&mut self) -> io::Result<(bool, String)>;
    /// Stop the command (reading only part of the output).
    fn kill(&mut self);
}

/// Runs CLI commands. `program` is `docker` or `kubectl`.
pub trait Runner: Send + Sync {
    /// Run to completion, feeding `stdin`. Fails with `TimedOut` after
    /// `timeout`, and with `NotFound` if the program isn't installed.
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> io::Result<CmdOutput>;
    /// Start a streaming command (stdin and stdout piped).
    fn spawn(&self, program: &str, args: &[String]) -> io::Result<Box<dyn Proc>>;
}

/// Directories where Docker Desktop, Homebrew, Rancher/OrbStack and
/// friends install their CLIs. Apps started from Finder get a minimal
/// `PATH`, so these are searched too.
fn extra_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = [
        "/usr/local/bin",
        "/opt/homebrew/bin",
        "/Applications/Docker.app/Contents/Resources/bin",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Some(h) = dirs::home_dir() {
        for d in [
            ".docker/bin",
            ".rd/bin",
            ".orbstack/bin",
            ".local/bin",
            "bin",
        ] {
            v.push(h.join(d));
        }
    }
    v
}

fn search_path() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for d in extra_dirs() {
        if !v.contains(&d) {
            v.push(d);
        }
    }
    v
}

/// Full path of an installed CLI, if any.
pub fn find_program(program: &str) -> Option<PathBuf> {
    search_path()
        .into_iter()
        .map(|d| d.join(program))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Set by [`block_cli_for_tests`]: [`CliRunner`] refuses to start anything.
static CLI_BLOCKED: AtomicBool = AtomicBool::new(false);

/// Env var that lets tests reach the real `docker` / `kubectl` CLIs.
pub const REAL_CLI_OPT_IN: &str = "MANTA_TEST_CONTAINERS";

/// `MANTA_TEST_CONTAINERS=1` is set: tests may use the real CLIs.
pub fn real_cli_opted_in() -> bool {
    std::env::var_os(REAL_CLI_OPT_IN).is_some_and(|v| v == "1")
}

/// Tests and renders call this so nothing can list or touch the
/// developer's own containers and pods: from now on [`CliRunner`] fails
/// with `PermissionDenied` (unless `MANTA_TEST_CONTAINERS=1`).
pub fn block_cli_for_tests() {
    CLI_BLOCKED.store(true, Ordering::SeqCst);
}

/// [`block_cli_for_tests`] is in effect (always in this crate's own unit
/// tests), and not overridden by `MANTA_TEST_CONTAINERS=1`.
pub fn cli_blocked() -> bool {
    (cfg!(test) || CLI_BLOCKED.load(Ordering::SeqCst)) && !real_cli_opted_in()
}

/// The real runner: spawns the CLI with a `PATH` that also contains the
/// usual install locations (credential helpers and kubectl auth plugins
/// live there too).
#[derive(Debug, Default, Clone, Copy)]
pub struct CliRunner;

impl CliRunner {
    pub fn shared() -> Arc<dyn Runner> {
        Arc::new(CliRunner)
    }

    fn command(program: &str, args: &[String]) -> io::Result<Command> {
        if cli_blocked() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{program} is disabled in tests (set {REAL_CLI_OPT_IN}=1)"),
            ));
        }
        let exe = find_program(program).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("{program} not found"))
        })?;
        let mut cmd = Command::new(exe);
        cmd.args(args);
        if let Ok(path) = std::env::join_paths(search_path()) {
            cmd.env("PATH", path);
        }
        Ok(cmd)
    }
}

/// Read a pipe to the end on a thread.
fn drain<R: Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut r) = r {
            let _ = r.read_to_end(&mut buf);
        }
        buf
    })
}

impl Runner for CliRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> io::Result<CmdOutput> {
        let mut cmd = Self::command(program, args)?;
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
            let data = data.to_vec();
            std::thread::spawn(move || {
                let _ = pipe.write_all(&data);
            });
        }
        let out = drain(child.stdout.take());
        let err = drain(child.stderr.take());
        let start = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break s;
            }
            if start.elapsed() > timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{program} did not answer within {} s", timeout.as_secs()),
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        Ok(CmdOutput {
            success: status.success(),
            stdout: out.join().unwrap_or_default(),
            stderr: String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned(),
        })
    }

    fn spawn(&self, program: &str, args: &[String]) -> io::Result<Box<dyn Proc>> {
        let mut cmd = Self::command(program, args)?;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        let stderr = drain(child.stderr.take());
        Ok(Box::new(CliProc {
            child,
            stderr: Some(stderr),
        }))
    }
}

struct CliProc {
    child: Child,
    stderr: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl Proc for CliProc {
    fn take_stdin(&mut self) -> Option<Box<dyn Write + Send>> {
        self.child
            .stdin
            .take()
            .map(|s| Box::new(s) as Box<dyn Write + Send>)
    }

    fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
        self.child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>)
    }

    fn wait(&mut self) -> io::Result<(bool, String)> {
        drop(self.child.stdin.take());
        let status = self.child.wait()?;
        let err = self
            .stderr
            .take()
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default();
        Ok((status.success(), String::from_utf8_lossy(&err).into_owned()))
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for CliProc {
    fn drop(&mut self) {
        // Never leave a CLI process behind.
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

// ------------------------------------------------------------------ errors

/// The CLI couldn't be started.
pub fn spawn_error(program: &str, e: &io::Error) -> VfsError {
    match e.kind() {
        io::ErrorKind::NotFound => VfsError::Connection(match program {
            "kubectl" => {
                "kubectl not found — install it (brew install kubectl) to browse pods".into()
            }
            _ => {
                "docker not found — install Docker Desktop (or the docker CLI) to browse containers"
                    .into()
            }
        }),
        io::ErrorKind::TimedOut => VfsError::Connection(e.to_string()),
        _ => VfsError::Connection(format!("cannot run {program}: {e}")),
    }
}

/// Map an error text of `docker` / `kubectl` (or of a command run inside
/// the container) to a friendly error. `what` names the path or object.
pub fn map_error(program: &str, stderr: &str, what: &str) -> VfsError {
    let first = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("command failed")
        .to_string();
    let e = stderr.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| e.contains(w));
    if has(&[
        "cannot connect to the docker daemon",
        "is the docker daemon running",
        "docker daemon is not running",
        "error during connect",
    ]) {
        return VfsError::Connection(
            "Docker daemon not running — start Docker Desktop and try again".into(),
        );
    }
    if program == "kubectl"
        && has(&[
            "unable to connect to the server",
            "current-context is not set",
            "no configuration has been provided",
            "context was not found",
            "connection refused",
            "dial tcp",
            "i/o timeout",
            "no such host",
        ])
    {
        return VfsError::Connection(format!("kubectl context unreachable: {first}"));
    }
    if has(&["no such container", "no such object"]) {
        return VfsError::NotFound(format!("container {what} not found (removed or renamed?)"));
    }
    if has(&["is not running", "is paused", "is restarting"]) {
        return VfsError::Connection(format!("container is not running: {first}"));
    }
    if program == "kubectl" && e.contains("pods ") && e.contains("not found") {
        return VfsError::NotFound(format!("pod not found: {first}"));
    }
    if has(&[
        "forbidden",
        "permission denied",
        "operation not permitted",
        "read-only file system",
    ]) {
        return VfsError::PermissionDenied(format!("{what}: {first}"));
    }
    if has(&[
        "no such file or directory",
        "could not find the file",
        "can't cd to",
        "can't stat",
        "not found",
    ]) {
        return VfsError::NotFound(what.to_string());
    }
    if has(&["file exists"]) {
        return VfsError::AlreadyExists(what.to_string());
    }
    if has(&["directory not empty"]) {
        return VfsError::DirectoryNotEmpty(what.to_string());
    }
    if has(&["not a directory"]) {
        return VfsError::NotADirectory(what.to_string());
    }
    VfsError::Other(format!("{what}: {first}"))
}

/// The shell (or `sh` itself) is missing in the container: distroless or
/// `scratch` images.
pub fn is_no_shell(stderr: &str) -> bool {
    let e = stderr.to_lowercase();
    e.contains("executable file not found")
        || (e.contains("oci runtime exec failed") && e.contains("no such file or directory"))
        || e.contains("exec: \"sh\"")
}

#[cfg(test)]
pub mod mock {
    //! A scripted runner for unit tests.

    use super::*;
    use std::sync::Mutex;

    type Handler = dyn Fn(&str, &[String], Option<&[u8]>) -> CmdOutput + Send + Sync;

    /// Answers every command with `handler`; records the calls and
    /// whatever was written to a spawned command's stdin.
    pub struct MockRunner {
        handler: Box<Handler>,
        pub calls: Mutex<Vec<Vec<String>>>,
        pub written: Arc<Mutex<Vec<u8>>>,
    }

    impl MockRunner {
        pub fn new(
            handler: impl Fn(&str, &[String], Option<&[u8]>) -> CmdOutput + Send + Sync + 'static,
        ) -> Arc<MockRunner> {
            Arc::new(MockRunner {
                handler: Box::new(handler),
                calls: Mutex::new(Vec::new()),
                written: Arc::default(),
            })
        }

        pub fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, program: &str, args: &[String]) {
            let mut v = vec![program.to_string()];
            v.extend(args.iter().cloned());
            self.calls.lock().unwrap().push(v);
        }
    }

    impl Runner for MockRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
            stdin: Option<&[u8]>,
            _: Duration,
        ) -> io::Result<CmdOutput> {
            self.record(program, args);
            Ok((self.handler)(program, args, stdin))
        }

        fn spawn(&self, program: &str, args: &[String]) -> io::Result<Box<dyn Proc>> {
            self.record(program, args);
            let out = (self.handler)(program, args, None);
            self.written.lock().unwrap().clear();
            Ok(Box::new(MockProc {
                out: Some(out),
                written: self.written.clone(),
            }))
        }
    }

    struct MockProc {
        out: Option<CmdOutput>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Proc for MockProc {
        fn take_stdin(&mut self) -> Option<Box<dyn Write + Send>> {
            Some(Box::new(SharedWriter(self.written.clone())))
        }
        fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
            let data = self
                .out
                .as_ref()
                .map(|o| o.stdout.clone())
                .unwrap_or_default();
            Some(Box::new(io::Cursor::new(data)))
        }
        fn wait(&mut self) -> io::Result<(bool, String)> {
            let o = self.out.take().unwrap_or_default();
            Ok((o.success, o.stderr))
        }
        fn kill(&mut self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_errors() {
        let daemon = "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?";
        assert!(
            matches!(map_error("docker", daemon, "x"), VfsError::Connection(m) if m.contains("daemon not running"))
        );
        assert!(
            matches!(map_error("docker", "Error response from daemon: No such container: web", "web"), VfsError::NotFound(m) if m.contains("container web"))
        );
        assert!(matches!(
            map_error("docker", "Error response from daemon: container 3f2a is not running", "/"),
            VfsError::Connection(m) if m.starts_with("container is not running")
        ));
        assert!(matches!(
            map_error(
                "docker",
                "rm: can't remove '/etc/x': Permission denied",
                "/etc/x"
            ),
            VfsError::PermissionDenied(_)
        ));
        assert!(
            matches!(map_error("docker", "Error response from daemon: Could not find the file /nope in container c", "/nope"), VfsError::NotFound(p) if p == "/nope")
        );
        assert!(matches!(
            map_error(
                "docker",
                "mkdir: can't create directory '/a': File exists",
                "/a"
            ),
            VfsError::AlreadyExists(_)
        ));
        assert!(matches!(
            map_error("docker", "rmdir: '/a': Directory not empty", "/a"),
            VfsError::DirectoryNotEmpty(_)
        ));
        assert!(matches!(
            map_error("kubectl", "Unable to connect to the server: dial tcp 127.0.0.1:6443: connect: connection refused", "pod"),
            VfsError::Connection(m) if m.starts_with("kubectl context unreachable")
        ));
        assert!(matches!(
            map_error("kubectl", "error: current-context is not set", "pod"),
            VfsError::Connection(m) if m.starts_with("kubectl context unreachable")
        ));
        assert!(
            matches!(map_error("kubectl", "Error from server (NotFound): pods \"web-0\" not found", "web-0"), VfsError::NotFound(m) if m.starts_with("pod not found"))
        );
        assert!(matches!(
            map_error(
                "kubectl",
                "Error from server (Forbidden): pods \"x\" is forbidden",
                "x"
            ),
            VfsError::PermissionDenied(_)
        ));
        assert!(
            matches!(map_error("docker", "weird", "/a"), VfsError::Other(m) if m == "/a: weird")
        );
        let nf = io::Error::new(io::ErrorKind::NotFound, "x");
        assert!(
            matches!(spawn_error("docker", &nf), VfsError::Connection(m) if m.starts_with("docker not found"))
        );
        assert!(
            matches!(spawn_error("kubectl", &nf), VfsError::Connection(m) if m.starts_with("kubectl not found"))
        );
    }

    #[test]
    fn detects_missing_shell() {
        assert!(is_no_shell("OCI runtime exec failed: exec failed: unable to start container process: exec: \"sh\": executable file not found in $PATH: unknown"));
        assert!(!is_no_shell("container is not running"));
    }
}
