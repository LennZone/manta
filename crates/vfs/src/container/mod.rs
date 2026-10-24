//! Docker containers and Kubernetes pods as folders, through the `docker`
//! and `kubectl` CLIs (no daemon API, no extra dependencies).
//!
//! - Metadata and changes run inside the container: `docker exec` /
//!   `kubectl exec` with a POSIX `sh` script or a plain command. Paths are
//!   always passed as separate arguments, never spliced into a script.
//! - Reading: `docker cp ctr:file -` (a tar stream; the single entry is
//!   extracted while streaming; works on stopped containers too), or
//!   `kubectl exec … cat file`.
//! - Writing: `exec -i … sh -c 'cat > "$1"'`, streaming stdin. (A tar for
//!   `docker cp -` needs the size before the content, which a writer
//!   doesn't know.) `flush()` finishes the upload and reports the exit
//!   status; writing again afterwards appends.
//! - Stopped containers and containers without a shell (distroless,
//!   `scratch`) open **read-only**: folders are listed from the tar of
//!   `docker cp ctr:dir -`, files read with `docker cp`.
//!
//! Every CLI call goes through a [`Runner`], with a timeout.

pub mod discover;
pub mod listing;
pub mod runner;

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::time::Duration;

use manta_core::{
    ConnectionConfig, ContainerOptions, Entry, Protocol, Vfs, VfsError, VfsPath, VfsResult,
};

pub use listing::sh_quote;
pub use runner::{
    block_cli_for_tests, cli_blocked, find_program, real_cli_opted_in, CliRunner, CmdOutput, Proc,
    Runner, REAL_CLI_OPT_IN,
};

use listing::{parse_listing, LIST_SCRIPT, TAR_LIST_LIMIT};
use runner::{map_error, spawn_error, COMMAND_TIMEOUT};

/// What the connection points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Docker {
        container: String,
    },
    Kube {
        context: String,
        namespace: String,
        pod: String,
        container: String,
    },
}

impl Target {
    pub fn from_config(cfg: &ConnectionConfig) -> VfsResult<Target> {
        let o = cfg.container.as_deref().cloned().unwrap_or_default();
        match cfg.protocol {
            Protocol::Docker => {
                let container = Some(o.container.trim())
                    .filter(|c| !c.is_empty())
                    .unwrap_or(cfg.host.trim())
                    .to_string();
                if container.is_empty() {
                    return Err(VfsError::Other("no container name given".into()));
                }
                Ok(Target::Docker { container })
            }
            Protocol::Kubernetes => {
                if o.pod.trim().is_empty() {
                    return Err(VfsError::Other("no pod name given".into()));
                }
                Ok(Target::Kube {
                    context: o.context.trim().into(),
                    namespace: o.namespace.trim().into(),
                    pod: o.pod.trim().into(),
                    container: o.container.trim().into(),
                })
            }
            p => Err(VfsError::Unsupported(format!(
                "{p} is not a container protocol"
            ))),
        }
    }

    pub fn program(&self) -> &'static str {
        match self {
            Target::Docker { .. } => "docker",
            Target::Kube { .. } => "kubectl",
        }
    }

    fn name(&self) -> &str {
        match self {
            Target::Docker { container } => container,
            Target::Kube { pod, .. } => pod,
        }
    }

    /// `kubectl` flags selecting context and namespace.
    fn kube_flags(context: &str, namespace: &str) -> Vec<String> {
        let mut v = Vec::new();
        if !context.is_empty() {
            v.extend(["--context".to_string(), context.to_string()]);
        }
        if !namespace.is_empty() {
            v.extend(["-n".to_string(), namespace.to_string()]);
        }
        v
    }

    /// Arguments that run `cmd` inside the container.
    pub fn exec_args(&self, interactive: bool, cmd: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        match self {
            Target::Docker { container } => {
                v.push("exec".into());
                if interactive {
                    v.push("-i".into());
                }
                v.push(container.clone());
            }
            Target::Kube {
                context,
                namespace,
                pod,
                container,
            } => {
                v.extend(Self::kube_flags(context, namespace));
                v.push("exec".into());
                if interactive {
                    v.push("-i".into());
                }
                v.push(pod.clone());
                if !container.is_empty() {
                    v.extend(["-c".to_string(), container.clone()]);
                }
                v.push("--".into());
            }
        }
        v.extend(cmd.iter().map(|s| s.to_string()));
        v
    }

    /// The command line that opens an interactive shell in `dir` (for the
    /// terminal; quoted for a POSIX shell).
    pub fn shell_command(&self, dir: &VfsPath) -> String {
        let script = format!("cd {} && exec ${{SHELL:-sh}}", sh_quote(dir.as_str()));
        match self {
            Target::Docker { container } => {
                format!(
                    "docker exec -it {} sh -c {}",
                    sh_quote(container),
                    sh_quote(&script)
                )
            }
            Target::Kube {
                context,
                namespace,
                pod,
                container,
            } => {
                let flags: Vec<String> = Self::kube_flags(context, namespace)
                    .iter()
                    .map(|f| sh_quote(f))
                    .collect();
                let flags = if flags.is_empty() {
                    String::new()
                } else {
                    format!(" {}", flags.join(" "))
                };
                let c = if container.is_empty() {
                    String::new()
                } else {
                    format!(" -c {}", sh_quote(container))
                };
                format!(
                    "kubectl{flags} exec -it {}{c} -- sh -c {}",
                    sh_quote(pod),
                    sh_quote(&script)
                )
            }
        }
    }
}

/// How the container can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Running with a shell: everything works.
    Exec,
    /// Stopped: read-only through `docker cp`.
    Stopped,
    /// No `sh` in the image: read-only through `docker cp`.
    NoShell,
}

/// A Docker container or a Kubernetes pod's container.
pub struct ContainerFs {
    runner: Arc<dyn Runner>,
    target: Target,
    label: String,
    home: VfsPath,
    mode: Mode,
}

/// The names the docs and the task use for the two flavours.
pub type DockerFs = ContainerFs;
pub type K8sFs = ContainerFs;

impl ContainerFs {
    /// Connect with the real CLIs. Blocking.
    pub fn connect(cfg: &ConnectionConfig) -> VfsResult<ContainerFs> {
        Self::connect_with(cfg, CliRunner::shared())
    }

    /// Check the container (running? shell?) and find its working dir.
    pub fn connect_with(cfg: &ConnectionConfig, runner: Arc<dyn Runner>) -> VfsResult<ContainerFs> {
        let target = Target::from_config(cfg)?;
        let mut fs = ContainerFs {
            runner,
            label: String::new(),
            home: VfsPath::root(),
            mode: Mode::Exec,
            target: target.clone(),
        };
        match &target {
            Target::Docker { container } => {
                let out = fs.cli(
                    &[
                        "inspect".into(),
                        "--type".into(),
                        "container".into(),
                        "--format".into(),
                        "{{.State.Running}}\t{{.Config.WorkingDir}}\t{{.Name}}".into(),
                        container.clone(),
                    ],
                    container,
                )?;
                let text = out.stdout_str();
                let mut parts = text.trim_end_matches('\n').split('\t');
                let running = parts.next() == Some("true");
                let workdir = parts.next().unwrap_or("").trim().to_string();
                let name = parts
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_start_matches('/')
                    .to_string();
                if !workdir.is_empty() {
                    fs.home = VfsPath::new(&workdir);
                }
                fs.mode = if running {
                    fs.probe_shell()?
                } else {
                    Mode::Stopped
                };
                let shown = if cfg.name.trim().is_empty() {
                    if name.is_empty() {
                        container.clone()
                    } else {
                        name
                    }
                } else {
                    cfg.name.trim().to_string()
                };
                fs.label = format!("{shown} (Docker)");
            }
            Target::Kube {
                context,
                namespace,
                pod,
                ..
            } => {
                let mut args = Target::kube_flags(context, namespace);
                args.extend(
                    ["get", "pod", pod, "-o", "jsonpath={.status.phase}"].map(String::from),
                );
                let phase = fs.cli(&args, pod)?.stdout_str().trim().to_string();
                if phase != "Running" {
                    return Err(VfsError::Connection(format!(
                        "pod {pod} is not running ({})",
                        if phase.is_empty() { "unknown" } else { &phase }
                    )));
                }
                if fs.probe_shell()? == Mode::NoShell {
                    return Err(VfsError::Unsupported(
                        "the container has no shell (sh) — Manta needs one to browse a pod".into(),
                    ));
                }
                let pwd = fs.exec(&["pwd"], pod)?.stdout_str();
                if pwd.trim().starts_with('/') {
                    fs.home = VfsPath::new(pwd.trim());
                }
                let shown = if cfg.name.trim().is_empty() {
                    match &target {
                        Target::Kube { container, .. } if !container.is_empty() => {
                            format!("{pod}/{container}")
                        }
                        _ => pod.clone(),
                    }
                } else {
                    cfg.name.trim().to_string()
                };
                fs.label = format!("{shown} (Kubernetes)");
            }
        }
        Ok(fs)
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// `Exec` if `sh` runs in the container, `NoShell` if it is missing.
    fn probe_shell(&self) -> VfsResult<Mode> {
        let args = self.target.exec_args(false, &["sh", "-c", "echo manta-ok"]);
        let out = self
            .runner
            .run(self.target.program(), &args, None, COMMAND_TIMEOUT)
            .map_err(|e| spawn_error(self.target.program(), &e))?;
        if out.success && out.stdout_str().contains("manta-ok") {
            Ok(Mode::Exec)
        } else if runner::is_no_shell(&out.stderr) {
            Ok(Mode::NoShell)
        } else {
            Err(map_error(
                self.target.program(),
                &out.stderr,
                self.target.name(),
            ))
        }
    }

    /// Run the CLI with `args`; errors are mapped with `what` as context.
    fn cli(&self, args: &[String], what: &str) -> VfsResult<CmdOutput> {
        let program = self.target.program();
        let out = self
            .runner
            .run(program, args, None, COMMAND_TIMEOUT)
            .map_err(|e| spawn_error(program, &e))?;
        if out.success {
            Ok(out)
        } else {
            Err(map_error(program, &out.stderr, what))
        }
    }

    /// Run `cmd` inside the container.
    fn exec(&self, cmd: &[&str], what: &str) -> VfsResult<CmdOutput> {
        self.cli(&self.target.exec_args(false, cmd), what)
    }

    fn writable(&self, path: &VfsPath) -> VfsResult<()> {
        match self.mode {
            Mode::Exec => Ok(()),
            Mode::Stopped => Err(VfsError::Unsupported(format!(
                "{path}: the container is stopped (read-only) — start it to make changes"
            ))),
            Mode::NoShell => Err(VfsError::Unsupported(format!(
                "{path}: the container has no shell (read-only)"
            ))),
        }
    }

    fn docker_name(&self) -> &str {
        match &self.target {
            Target::Docker { container } => container,
            Target::Kube { pod, .. } => pod,
        }
    }

    /// Start `docker cp [-L] ctr:path -` and return its stdout.
    fn docker_cp_out(
        &self,
        path: &VfsPath,
        follow: bool,
    ) -> VfsResult<(Box<dyn Proc>, Box<dyn Read + Send>)> {
        let mut args = vec!["cp".to_string()];
        if follow {
            args.push("-L".into());
        }
        args.push(format!("{}:{}", self.docker_name(), path.as_str()));
        args.push("-".into());
        let mut p = self
            .runner
            .spawn("docker", &args)
            .map_err(|e| spawn_error("docker", &e))?;
        let out = p
            .take_stdout()
            .ok_or_else(|| VfsError::Other("docker cp: no output".into()))?;
        Ok((p, out))
    }

    /// Finish a `docker cp` and turn its exit into the error to report,
    /// if any.
    fn cp_status(p: &mut Box<dyn Proc>, path: &VfsPath) -> VfsResult<()> {
        match p.wait() {
            Ok((true, _)) => Ok(()),
            Ok((false, err)) => Err(map_error("docker", &err, path.as_str())),
            Err(e) => Err(VfsError::Io(e)),
        }
    }

    fn list_exec(&self, mode: &str, path: &VfsPath, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        let out = self.exec(
            &["sh", "-c", LIST_SCRIPT, "sh", mode, path.as_str()],
            path.as_str(),
        )?;
        parse_listing(&out.stdout, dir)
    }
}

impl Vfs for ContainerFs {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn protocol(&self) -> Protocol {
        match self.target {
            Target::Docker { .. } => Protocol::Docker,
            Target::Kube { .. } => Protocol::Kubernetes,
        }
    }

    fn home(&self) -> VfsResult<VfsPath> {
        Ok(self.home.clone())
    }

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        if self.mode == Mode::Exec {
            return self.list_exec("L", dir, dir);
        }
        let (mut p, out) = self.docker_cp_out(dir, false)?;
        let listed = listing::list_tar(out, dir, TAR_LIST_LIMIT);
        if listed.is_err() {
            p.kill();
        }
        let status = Self::cp_status(&mut p, dir);
        let entries = listed?;
        status?;
        Ok(entries)
    }

    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        if self.mode == Mode::Exec {
            let parent = path.parent().unwrap_or_else(VfsPath::root);
            return self
                .list_exec("S", path, &parent)?
                .into_iter()
                .next()
                .ok_or_else(|| VfsError::NotFound(path.to_string()));
        }
        let (mut p, out) = self.docker_cp_out(path, false)?;
        let e = listing::stat_tar(out, path);
        // Only the first header is needed.
        p.kill();
        match e {
            Ok(e) => Ok(e),
            Err(err) => match p.wait() {
                Ok((false, stderr)) if !stderr.is_empty() => {
                    Err(map_error("docker", &stderr, path.as_str()))
                }
                _ => Err(err),
            },
        }
    }

    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.writable(path)?;
        self.exec(&["mkdir", path.as_str()], path.as_str())
            .map(drop)
    }

    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        self.writable(path)?;
        self.exec(&["rm", "-f", path.as_str()], path.as_str())
            .map(drop)
    }

    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.writable(path)?;
        self.exec(&["rmdir", path.as_str()], path.as_str())
            .map(drop)
    }

    fn remove_tree(&self, path: &VfsPath) -> VfsResult<()> {
        self.writable(path)?;
        if path.is_root() {
            return Err(VfsError::PermissionDenied(
                "refusing to delete / of a container".into(),
            ));
        }
        self.exec(&["rm", "-rf", path.as_str()], path.as_str())
            .map(drop)
    }

    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        self.writable(from)?;
        // `mv a b` moves *into* b when b is a folder: refuse that instead.
        let script = r#"if [ -d "$2" ] && [ ! -L "$2" ]; then echo "$2: File exists" >&2; exit 1; fi; exec mv -f "$1" "$2""#;
        self.exec(
            &["sh", "-c", script, "sh", from.as_str(), to.as_str()],
            to.as_str(),
        )
        .map(drop)
    }

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        match &self.target {
            Target::Docker { .. } => {
                let (mut p, out) = self.docker_cp_out(path, true)?;
                match listing::TarFileReader::new(out, path) {
                    Ok(r) => Ok(Box::new(ProcReader {
                        inner: r,
                        proc: Some(p),
                        path: path.clone(),
                        tar: true,
                        program: "docker",
                    })),
                    Err(e) => {
                        p.kill();
                        match p.wait() {
                            Ok((false, stderr)) if !stderr.is_empty() => {
                                Err(map_error("docker", &stderr, path.as_str()))
                            }
                            _ => Err(e),
                        }
                    }
                }
            }
            Target::Kube { .. } => {
                let e = self.stat(path)?;
                if e.is_dir() {
                    return Err(VfsError::Other(format!("{path} is a folder")));
                }
                let args = self.target.exec_args(false, &["cat", path.as_str()]);
                let mut p = self
                    .runner
                    .spawn("kubectl", &args)
                    .map_err(|e| spawn_error("kubectl", &e))?;
                let out = p
                    .take_stdout()
                    .ok_or_else(|| VfsError::Other("kubectl: no output".into()))?;
                Ok(Box::new(ProcReader {
                    inner: out,
                    proc: Some(p),
                    path: path.clone(),
                    tar: false,
                    program: "kubectl",
                }))
            }
        }
    }

    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        self.writable(path)?;
        let mut w = ExecWriter {
            runner: self.runner.clone(),
            target: self.target.clone(),
            path: path.clone(),
            proc: None,
            stdin: None,
            started: false,
        };
        w.start(false)?;
        Ok(Box::new(w))
    }

    /// `tail -c +N` inside the container (POSIX, busybox too). Stopped
    /// containers and containers without a shell use the default (read and
    /// discard).
    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        if offset == 0 || self.mode != Mode::Exec {
            let mut r = self.open_read(path)?;
            if offset > 0 {
                let skipped = io::copy(&mut (&mut r).take(offset), &mut io::sink())?;
                if skipped < offset {
                    return Err(VfsError::Other(format!(
                        "{path}: shorter than the resume offset {offset}"
                    )));
                }
            }
            return Ok(r);
        }
        // Check first so a missing file / folder gives a proper error
        // instead of an empty stream.
        let e = self.stat(path)?;
        if e.is_dir() {
            return Err(VfsError::Other(format!("{path} is a folder")));
        }
        if offset > e.size {
            return Err(VfsError::Other(format!(
                "{path}: file is shorter ({} bytes) than the resume offset {offset}",
                e.size
            )));
        }
        let start = (offset + 1).to_string();
        let args = self.target.exec_args(
            false,
            &["sh", "-c", READ_AT_SCRIPT, "sh", path.as_str(), &start],
        );
        let program = self.target.program();
        let mut p = self
            .runner
            .spawn(program, &args)
            .map_err(|e| spawn_error(program, &e))?;
        let out = p
            .take_stdout()
            .ok_or_else(|| VfsError::Other(format!("{program}: no output")))?;
        Ok(Box::new(ProcReader {
            inner: out,
            proc: Some(p),
            path: path.clone(),
            tar: false,
            program,
        }))
    }

    /// `cat >> "$1"` inside the container.
    fn open_append(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        self.writable(path)?;
        let mut w = ExecWriter {
            runner: self.runner.clone(),
            target: self.target.clone(),
            path: path.clone(),
            proc: None,
            stdin: None,
            started: false,
        };
        w.start(true)?;
        Ok(Box::new(w))
    }

    fn supports_resume(&self) -> manta_core::ResumeCaps {
        let exec = self.mode == Mode::Exec;
        manta_core::ResumeCaps {
            read_at: exec,
            append: exec,
            upload_session: false,
        }
    }

    /// The CLI talks to the daemon / cluster per call: just check the
    /// container answers again.
    fn reconnect(&self) -> VfsResult<()> {
        match self.mode {
            Mode::Exec => self.probe_shell().map(drop),
            Mode::Stopped | Mode::NoShell => Ok(()),
        }
    }

    fn set_permissions(&self, path: &VfsPath, mode: u32) -> VfsResult<()> {
        self.writable(path)?;
        let m = format!("{:o}", mode & 0o7777);
        self.exec(&["chmod", &m, path.as_str()], path.as_str())
            .map(drop)
    }

    fn set_modified(&self, path: &VfsPath, mtime: chrono::DateTime<chrono::Utc>) -> VfsResult<()> {
        self.writable(path)?;
        let epoch = format!("@{}", mtime.timestamp());
        let stamp = mtime.format("%Y%m%d%H%M.%S").to_string();
        // GNU and newer busybox take `-d @epoch`; older busybox `-t`.
        let script = r#"touch -c -d "$1" "$3" 2>/dev/null || TZ=UTC0 touch -c -t "$2" "$3""#;
        self.exec(
            &["sh", "-c", script, "sh", &epoch, &stamp, path.as_str()],
            path.as_str(),
        )
        .map(drop)
    }

    fn is_read_only(&self) -> bool {
        self.mode != Mode::Exec
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// Reads a streaming command's stdout; at the end, checks its exit.
struct ProcReader<R> {
    inner: R,
    proc: Option<Box<dyn Proc>>,
    path: VfsPath,
    /// `docker cp` (the tar may have trailing blocks: stop the process).
    tar: bool,
    /// CLI name for error messages.
    program: &'static str,
}

impl<R: Read> Read for ProcReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n == 0 && !buf.is_empty() {
            if let Some(mut p) = self.proc.take() {
                if self.tar {
                    // The file is complete; the rest is tar padding.
                    p.kill();
                    return Ok(0);
                }
                let (ok, err) = p.wait()?;
                if !ok {
                    let e = map_error(self.program, &err, self.path.as_str());
                    return Err(io::Error::other(e.to_string()));
                }
            }
        }
        Ok(n)
    }
}

impl<R> Drop for ProcReader<R> {
    fn drop(&mut self) {
        if let Some(mut p) = self.proc.take() {
            p.kill();
        }
    }
}

/// Streams into `exec -i … sh -c 'cat > "$1"'`.
struct ExecWriter {
    runner: Arc<dyn Runner>,
    target: Target,
    path: VfsPath,
    proc: Option<Box<dyn Proc>>,
    stdin: Option<Box<dyn Write + Send>>,
    started: bool,
}

const WRITE_SCRIPT: &str = r#"cat > "$1""#;
/// Read from byte `$2` (1-based) on: `tail -c +N` is POSIX and in busybox;
/// `dd` as a fallback for tails without `-c`.
const READ_AT_SCRIPT: &str = r#"if tail -c +1 /dev/null >/dev/null 2>&1; then exec tail -c +"$2" "$1"; else exec dd if="$1" bs=1 skip="$(($2 - 1))" 2>/dev/null; fi"#;
const APPEND_SCRIPT: &str = r#"cat >> "$1""#;

impl ExecWriter {
    fn start(&mut self, append: bool) -> VfsResult<()> {
        let script = if append { APPEND_SCRIPT } else { WRITE_SCRIPT };
        let args = self
            .target
            .exec_args(true, &["sh", "-c", script, "sh", self.path.as_str()]);
        let program = self.target.program();
        let mut p = self
            .runner
            .spawn(program, &args)
            .map_err(|e| spawn_error(program, &e))?;
        self.stdin = p.take_stdin();
        self.proc = Some(p);
        self.started = true;
        Ok(())
    }

    fn finish(&mut self) -> VfsResult<()> {
        drop(self.stdin.take());
        let Some(mut p) = self.proc.take() else {
            return Ok(());
        };
        match p.wait()? {
            (true, _) => Ok(()),
            (false, err) => Err(map_error(self.target.program(), &err, self.path.as_str())),
        }
    }
}

impl Drop for ExecWriter {
    fn drop(&mut self) {
        // Dropped without flush(): finish the upload anyway.
        let _ = self.finish();
    }
}

fn to_io(e: VfsError) -> io::Error {
    match e {
        VfsError::Io(e) => e,
        e => io::Error::other(e.to_string()),
    }
}

impl Write for ExecWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.proc.is_none() {
            // Written again after `flush()`: append.
            self.start(self.started).map_err(to_io)?;
        }
        let Some(w) = self.stdin.as_mut() else {
            return Err(io::Error::other("upload stream closed"));
        };
        match w.write(buf) {
            Ok(n) => Ok(n),
            // The command died (e.g. permission denied): report its error.
            Err(e) => Err(match self.finish() {
                Err(v) => to_io(v),
                Ok(()) => e,
            }),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.finish().map_err(to_io)
    }
}

/// Settings for a Docker container (by name, so a favorite survives
/// re-creation of the container).
pub fn docker_config(container: &str) -> ConnectionConfig {
    ConnectionConfig {
        name: String::new(),
        protocol: Protocol::Docker,
        host: container.to_string(),
        port: 0,
        username: String::new(),
        auth: manta_core::Auth::None,
        initial_path: None,
        s3: None,
        container: Some(Box::new(ContainerOptions {
            container: container.to_string(),
            ..Default::default()
        })),
        ftp: None,
        webdav: None,
        smb: None,
    }
}

/// Settings for a container of a Kubernetes pod.
pub fn kube_config(context: &str, namespace: &str, pod: &str, container: &str) -> ConnectionConfig {
    ConnectionConfig {
        name: String::new(),
        protocol: Protocol::Kubernetes,
        host: if namespace.is_empty() {
            pod.to_string()
        } else {
            format!("{namespace}/{pod}")
        },
        port: 0,
        username: container.to_string(),
        auth: manta_core::Auth::None,
        initial_path: None,
        s3: None,
        container: Some(Box::new(ContainerOptions {
            container: container.to_string(),
            context: context.to_string(),
            namespace: namespace.to_string(),
            pod: pod.to_string(),
        })),
        ftp: None,
        webdav: None,
        smb: None,
    }
}

/// Default timeout re-exported for callers that run discovery.
pub const DISCOVERY_TIMEOUT: Duration = runner::DISCOVERY_TIMEOUT;

#[cfg(test)]
mod tests;
