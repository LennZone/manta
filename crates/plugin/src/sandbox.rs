//! The macOS sandbox for external plugins: the declared `permissions`
//! become rules the kernel enforces.
//!
//! An external plugin's process is started through `/usr/bin/sandbox-exec`
//! with a generated profile (SBPL, the Scheme dialect of Apple's Seatbelt).
//! The profile denies everything by default and then allows:
//!
//! - **always**: running the declared command (the program, its canonical
//!   path and a script's `#!` interpreter), reading the plugin's own folder,
//!   reading and writing its data folder (`MANTA_PLUGIN_DATA_DIR`, also the
//!   process's `TMPDIR`), reading the system (`/System`, `/usr`, `/bin`,
//!   `/Library/Preferences`, Homebrew in `/opt/homebrew` and `/usr/local`,
//!   the Xcode / Command Line Tools developer folder …) and the install
//!   folder of the interpreter (e.g. `~/.nvm/versions/node/v22`), file
//!   metadata (names, sizes and dates, needed to resolve paths) and a few
//!   system services (user lookups, logging, preferences);
//! - `read_files`: reading every file (`file-read*`);
//! - `write_files`: writing every file (`file-write*`);
//! - `network`: network access (`network*` and DNS / TLS services);
//! - `run_programs`: starting any program (`process-exec*`);
//! - `terminal`: nothing here (`run_in_terminal` goes through Manta, which
//!   checks it; see [`crate::host`]).
//!
//! `sandbox-exec` is deprecated but still works on macOS 26 (checked by
//! [`available`] with a probe run, once per launch). Where it doesn't, the
//! plugin runs without the sandbox and Settings and its log say so.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The tool that applies a profile and runs a program in it.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Whether a plugin runs in the sandbox (shown in Settings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxState {
    /// The declared permissions are enforced.
    Enforced,
    /// The sandbox can't be used here; the plugin runs with the user's
    /// rights. The text says why (technical English).
    Unavailable(String),
}

/// What a plugin may do beyond the basics, from its `permissions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Policy {
    pub read_files: bool,
    pub write_files: bool,
    pub network: bool,
    pub run_programs: bool,
}

impl Policy {
    /// Unknown permission names grant nothing.
    pub fn from_permissions(perms: &[String]) -> Policy {
        let has = |p: &str| perms.iter().any(|x| x == p);
        Policy {
            read_files: has("read_files"),
            write_files: has("write_files"),
            network: has("network"),
            run_programs: has("run_programs"),
        }
    }
}

/// Folders every plugin may read (programs, libraries, system settings).
const SYSTEM_READ: &[&str] = &[
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/Library/Apple",
    "/Library/Preferences",
    "/Library/Frameworks",
    "/Library/Developer/CommandLineTools",
    "/private/etc",
    "/private/var/db/dyld",
    "/private/var/db/timezone",
    "/private/var/select",
    "/dev",
    "/opt/homebrew",
    "/opt/local",
    "/usr/local",
];

/// Single files every plugin may read.
const SYSTEM_READ_FILES: &[&str] = &["/", "/private/var/db/xcode_select_link"];

/// Mach services every plugin may talk to: user and group lookups,
/// logging, notifications, temporary folders, preferences.
const BASE_SERVICES: &[&str] = &[
    "com.apple.system.opendirectoryd.libinfo",
    "com.apple.system.opendirectoryd.membership",
    "com.apple.system.logger",
    "com.apple.system.notification_center",
    "com.apple.logd",
    "com.apple.bsd.dirhelper",
    "com.apple.cfprefsd.daemon",
    "com.apple.cfprefsd.agent",
];

/// Extra services for `network`: name resolution, network configuration,
/// certificate checks.
const NETWORK_SERVICES: &[&str] = &[
    "com.apple.dnssd.service",
    "com.apple.mDNSResponder",
    "com.apple.SystemConfiguration.configd",
    "com.apple.SystemConfiguration.DNSConfiguration",
    "com.apple.networkd",
    "com.apple.nehelper",
    "com.apple.symptomsd",
    "com.apple.usymptomsd",
    "com.apple.trustd",
    "com.apple.trustd.agent",
    "com.apple.ocspd",
];

/// Shells `/bin/sh` hands scripts to (it is a small launcher on macOS).
const SH_TARGETS: &[&str] = &["/bin/bash", "/bin/zsh", "/bin/dash"];

/// Everything [`profile`] needs besides the policy.
#[derive(Debug, Clone, Default)]
pub struct Paths {
    /// The plugin folder (read only).
    pub plugin_dir: PathBuf,
    /// Its data folder (read and write).
    pub data_dir: PathBuf,
    /// Programs the plugin may start without `run_programs`: the declared
    /// command and its interpreter ([`exec_targets`]).
    pub exec: Vec<PathBuf>,
    /// Folders it may read and run programs from: the interpreter's
    /// install folder, the developer folder for `/usr/bin` tool shims.
    pub tool_dirs: Vec<PathBuf>,
    /// The user's home folder (`~/.CFUserTextEncoding` is readable).
    pub home: Option<PathBuf>,
}

/// An SBPL string literal.
fn quote(p: &Path) -> String {
    let s = p.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn subpaths<'a>(paths: impl IntoIterator<Item = &'a Path>) -> String {
    paths
        .into_iter()
        .map(|p| format!(" (subpath {})", quote(p)))
        .collect()
}

fn literals<'a>(paths: impl IntoIterator<Item = &'a Path>) -> String {
    paths
        .into_iter()
        .map(|p| format!(" (literal {})", quote(p)))
        .collect()
}

fn services(names: &[&str]) -> String {
    names
        .iter()
        .map(|n| format!(" (global-name \"{n}\")"))
        .collect()
}

/// The sandbox profile for a plugin (see the module docs).
pub fn profile(policy: Policy, paths: &Paths) -> String {
    let mut p = String::from(
        ";; Generated by Manta for an external plugin (see docs/plugins.md, Security).\n\
         (version 1)\n\
         (deny default)\n\
         (allow process-fork)\n\
         (allow signal (target same-sandbox))\n\
         (allow sysctl-read)\n\
         (allow user-preference-read)\n\
         ;; Names, sizes and dates (resolving paths); not file contents.\n\
         (allow file-read-metadata)\n",
    );
    p += &format!("(allow mach-lookup{})\n", services(BASE_SERVICES));
    p += "(allow ipc-posix-shm-read-data (ipc-posix-name \"apple.shm.notification_center\") \
          (ipc-posix-name-regex #\"^apple\\.cfprefs\\.\"))\n";
    // The system, the plugin and its data.
    let system: Vec<PathBuf> = SYSTEM_READ.iter().map(PathBuf::from).collect();
    p += &format!(
        "(allow file-read*{})\n",
        subpaths(system.iter().map(PathBuf::as_path))
    );
    let mut files: Vec<PathBuf> = SYSTEM_READ_FILES.iter().map(PathBuf::from).collect();
    if let Some(home) = &paths.home {
        files.push(home.join(".CFUserTextEncoding"));
    }
    p += &format!(
        "(allow file-read*{})\n",
        literals(files.iter().map(PathBuf::as_path))
    );
    p += &format!(
        "(allow file-read*{})\n",
        subpaths([paths.plugin_dir.as_path(), paths.data_dir.as_path()])
    );
    if !paths.tool_dirs.is_empty() {
        p += &format!(
            "(allow file-read* process-exec*{})\n",
            subpaths(paths.tool_dirs.iter().map(PathBuf::as_path))
        );
    }
    p += &format!("(allow file-write* (subpath {}))\n", quote(&paths.data_dir));
    p += "(allow file-write-data file-ioctl (literal \"/dev/null\") (literal \"/dev/zero\") \
          (literal \"/dev/dtracehelper\"))\n";
    // xcrun (behind /usr/bin/python3, git …) caches its lookups here.
    p += "(allow file-read* file-write* \
          (regex #\"^/private/(tmp|var/folders/[^/]+/[^/]+/[CT])/xcrun_db\"))\n";
    // The bash that ships with macOS (3.2) ignores TMPDIR for here
    // documents (`<<EOF`, `<<<`) and makes them in /tmp, after checking
    // that /tmp is writable (only these files can be created there).
    p += "(allow file-read* file-write* (regex #\"^/private/tmp/sh-thd\"))\n\
          (allow file-write-data (literal \"/private/tmp\"))\n";
    if !paths.exec.is_empty() {
        p += &format!(
            "(allow process-exec*{})\n",
            literals(paths.exec.iter().map(PathBuf::as_path))
        );
        p += &format!(
            "(allow file-read*{})\n",
            literals(paths.exec.iter().map(PathBuf::as_path))
        );
    }
    // Declared permissions.
    if policy.read_files {
        p += ";; read_files\n(allow file-read*)\n";
    }
    if policy.write_files {
        p += ";; write_files\n(allow file-write*)\n";
    }
    if policy.network {
        p += &format!(
            ";; network\n(allow network*)\n(allow system-socket)\n(allow mach-lookup{})\n",
            services(NETWORK_SERVICES)
        );
    }
    if policy.run_programs {
        p += ";; run_programs\n(allow process-exec*)\n";
    }
    p
}

// ------------------------------------------------------------------ programs

/// Find `program` like `execvp` does: a name with `/` relative to `dir`
/// (the plugin folder, its working directory), a bare name on `path_var`.
pub fn resolve_program(program: &str, dir: &Path, path_var: &OsStr) -> Option<PathBuf> {
    if program.contains('/') {
        let p = Path::new(program);
        let p = if p.is_absolute() {
            p.to_path_buf()
        } else {
            dir.join(p)
        };
        return p.is_file().then_some(p);
    }
    std::env::split_paths(path_var)
        .map(|d| d.join(program))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    p.is_file()
}

/// The `#!` line of a script: interpreter and its first argument.
fn shebang(path: &Path) -> Option<(PathBuf, Option<String>)> {
    use std::io::Read;
    let mut buf = [0u8; 256];
    let n = std::fs::File::open(path).ok()?.read(&mut buf).ok()?;
    let head = std::str::from_utf8(&buf[..n]).ok().or_else(|| {
        // Cut at the last full character.
        (1..4).find_map(|k| std::str::from_utf8(&buf[..n.saturating_sub(k)]).ok())
    })?;
    let line = head.strip_prefix("#!")?.lines().next()?.trim();
    let mut parts = line.split_whitespace();
    let interp = PathBuf::from(parts.next()?);
    Some((interp, parts.next().map(str::to_string)))
}

/// The programs a plugin's command really starts: the program (as found
/// and its canonical path), a script's interpreter (through
/// `/usr/bin/env`, too) and the shells behind `/bin/sh`.
pub fn exec_targets(program: &Path, path_var: &OsStr) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let add = |p: PathBuf, out: &mut Vec<PathBuf>| {
        if let Ok(c) = std::fs::canonicalize(&p) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        if !out.contains(&p) {
            out.push(p);
        }
    };
    add(program.to_path_buf(), &mut out);
    if let Some((interp, arg)) = shebang(program) {
        if interp == Path::new("/usr/bin/env") {
            add(interp, &mut out);
            if let Some(name) = arg.filter(|a| !a.starts_with('-')) {
                if let Some(p) = resolve_program(&name, Path::new("/"), path_var) {
                    add(p, &mut out);
                }
            }
        } else {
            add(interp, &mut out);
        }
    }
    if out.iter().any(|p| p == Path::new("/bin/sh")) {
        for s in SH_TARGETS {
            add(PathBuf::from(s), &mut out);
        }
    }
    out
}

/// The active developer folder (`xcode-select -p`): Xcode's, if chosen.
fn developer_dir() -> Option<PathBuf> {
    // DEVELOPER_DIR wins over xcode-select (CI runners use it).
    let link = std::env::var_os("DEVELOPER_DIR")
        .map(PathBuf::from)
        .or_else(|| std::fs::read_link("/private/var/db/xcode_select_link").ok())?;
    // The sandbox matches real paths: resolve symlinks (Xcode.app → Xcode_15.4.app).
    let link = std::fs::canonicalize(&link).unwrap_or(link);
    // ".../Xcode.app/Contents/Developer": allow the whole app (its tools
    // load frameworks from Contents/SharedFrameworks).
    let app = link
        .ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf);
    Some(app.unwrap_or(link))
}

/// A folder that is too broad to open up as an interpreter's install
/// folder: `/`, top-level folders, the home folder and its direct children.
fn too_broad(dir: &Path, home: Option<&Path>) -> bool {
    let depth = dir.components().count();
    if depth <= 2 {
        return true;
    }
    match home {
        Some(h) => dir == h || dir.parent() == Some(h) || h.starts_with(dir),
        None => false,
    }
}

/// Read and run access the command needs beyond [`SYSTEM_READ`]: the
/// install folder (`<prefix>/bin/<program>` → `<prefix>`) of the real
/// (canonical) program, where interpreters keep their libraries and helper
/// programs (nvm, pyenv, a Homebrew or python.org framework Python, which
/// starts `Python.app` from there), and the developer folder when a
/// `/usr/bin` tool shim (python3, git …) is run. Never a folder that
/// contains a whole system area (`/usr`, `/opt/homebrew`), the home folder
/// or the plugins folder.
pub fn tool_dirs(exec: &[PathBuf], plugin_dir: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in exec {
        if p.starts_with("/usr/bin") {
            if let Some(d) = developer_dir() {
                if !out.contains(&d) {
                    out.push(d);
                }
            }
        }
        // Symlinks (`/opt/homebrew/bin/python3`) are listed with their
        // target; only the target's folder counts.
        let canonical = std::fs::canonicalize(p).map_or(true, |c| c == *p);
        if !canonical || p.starts_with(plugin_dir) {
            continue;
        }
        let Some(prefix) = p.parent().and_then(Path::parent) else {
            continue;
        };
        let covers_system_area = SYSTEM_READ.iter().any(|s| Path::new(s).starts_with(prefix));
        if too_broad(prefix, home) || covers_system_area || plugin_dir.starts_with(prefix) {
            continue;
        }
        let prefix = prefix.to_path_buf();
        if !out.contains(&prefix) {
            out.push(prefix);
        }
    }
    out
}

// ------------------------------------------------------------------ running

/// Whether `sandbox-exec` works here (probed once per launch).
pub fn available() -> Result<(), String> {
    static PROBE: OnceLock<Result<(), String>> = OnceLock::new();
    PROBE.get_or_init(|| probe(Path::new(SANDBOX_EXEC))).clone()
}

/// Run `tool` with a profile that allows everything; it must exit 0.
pub fn probe(tool: &Path) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("the plugin sandbox needs macOS".into());
    }
    if !tool.is_file() {
        return Err(format!("{} is missing", tool.display()));
    }
    let out = Command::new(tool)
        .args(["-p", "(version 1)(allow default)", "/usr/bin/true"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("{} can't be started: {e}", tool.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(format!(
            "{} doesn't work here ({}){}",
            tool.display(),
            out.status,
            if err.is_empty() {
                String::new()
            } else {
                format!(": {err}")
            }
        ))
    }
}

/// How to start a plugin.
pub struct Launch {
    /// `sandbox-exec -p <profile> <program> <args…>`, or the program itself
    /// when the sandbox is unavailable.
    pub command: Command,
    pub state: SandboxState,
    /// The generated profile (for the log; `None` without sandbox).
    pub profile: Option<String>,
}

/// Build the command for a plugin. `argv` is the manifest's `command`,
/// `dir` the plugin folder (also the working directory), `path_var` the
/// child's `PATH`. `tool` is [`SANDBOX_EXEC`] (tests: something else) and
/// `availability` the result of [`available`] / [`probe`].
///
/// Fails with `NotFound` when the program can't be found.
pub fn command(
    argv: &[String],
    dir: &Path,
    data_dir: &Path,
    permissions: &[String],
    path_var: &OsStr,
    tool: &Path,
    availability: Result<(), String>,
) -> std::io::Result<Launch> {
    let program = argv
        .first()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty command"))?;
    let Some(resolved) = resolve_program(program, dir, path_var) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("program not found: {program}"),
        ));
    };
    if let Err(why) = availability {
        let mut c = Command::new(&resolved);
        c.args(&argv[1..]);
        return Ok(Launch {
            command: c,
            state: SandboxState::Unavailable(why),
            profile: None,
        });
    }
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let plugin_dir = canon(dir);
    let _ = std::fs::create_dir_all(data_dir);
    let data_dir = canon(data_dir);
    let home = dirs::home_dir().map(|h| canon(&h));
    let exec = exec_targets(&resolved, path_var);
    let tool_dirs = tool_dirs(&exec, &plugin_dir, home.as_deref());
    let paths = Paths {
        plugin_dir,
        data_dir,
        exec,
        tool_dirs,
        home,
    };
    let text = profile(Policy::from_permissions(permissions), &paths);
    let mut c = Command::new(tool);
    c.arg("-p").arg(&text).arg(&resolved).args(&argv[1..]);
    Ok(Launch {
        command: c,
        state: SandboxState::Enforced,
        profile: Some(text),
    })
}

/// Environment for a sandboxed plugin: its own temporary folder inside the
/// data folder (the system one isn't writable), no Python byte-code files
/// in the read-only plugin folder.
pub fn env(data_dir: &Path) -> Vec<(OsString, OsString)> {
    let tmp = data_dir.join("tmp");
    let _ = std::fs::create_dir_all(&tmp);
    vec![
        ("TMPDIR".into(), tmp.into_os_string()),
        ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
    ]
}

#[cfg(test)]
mod tests;
