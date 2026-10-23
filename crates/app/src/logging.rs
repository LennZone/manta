//! Diagnostics log: everything the terminal logger shows also goes to a
//! file (`~/Library/Logs/Manta/manta.log` on macOS), which rotates at
//! about 5 MB and keeps one backup (`manta.log.1`).
//!
//! The level is `info` by default; `MANTA_LOG` (or `RUST_LOG`) takes an
//! env_logger filter spec such as `debug` or `info,manta=trace`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Rotate when the file would grow past this.
pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Default filter: info, but keep the GPU stack quiet.
const DEFAULT_FILTER: &str = "info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn";

/// Where the log file lives.
pub fn log_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| h.join("Library/Logs/Manta/manta.log"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        dirs::data_local_dir().map(|d| d.join("Manta").join("logs").join("manta.log"))
    }
}

/// An append-only file that is moved to `<name>.1` when it reaches `max` bytes.
pub struct RotatingFile {
    path: PathBuf,
    max: u64,
    file: Option<File>,
    size: u64,
}

impl RotatingFile {
    pub fn open(path: &Path, max: u64) -> std::io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(RotatingFile {
            path: path.to_path_buf(),
            max,
            file: Some(file),
            size,
        })
    }

    pub fn backup_path(&self) -> PathBuf {
        backup_path(&self.path)
    }

    pub fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        let len = line.len() as u64 + 1;
        if self.size > 0 && self.size + len > self.max {
            self.rotate()?;
        }
        let Some(f) = self.file.as_mut() else {
            return Ok(());
        };
        writeln!(f, "{line}")?;
        self.size += len;
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file = None; // close before renaming
        let _ = std::fs::rename(&self.path, self.backup_path());
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.size = file.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = Some(file);
        Ok(())
    }

    pub fn flush(&mut self) {
        if let Some(f) = self.file.as_mut() {
            let _ = f.flush();
        }
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".1");
    path.with_file_name(name)
}

/// Terminal (env_logger) plus file.
struct CombinedLogger {
    term: env_logger::Logger,
    file: Option<Mutex<RotatingFile>>,
}

impl log::Log for CombinedLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.term.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if !self.term.matches(record) {
            return;
        }
        self.term.log(record);
        if let Some(file) = &self.file {
            let line = format_line(
                &chrono::Local::now()
                    .format("%Y-%m-%d %H:%M:%S%.3f")
                    .to_string(),
                record.level(),
                record.target(),
                &record.args().to_string(),
            );
            if let Ok(mut f) = file.lock() {
                let _ = f.write_line(&line);
                // Errors are rare and the app may crash right after: flush.
                if record.level() <= log::Level::Warn {
                    f.flush();
                }
            }
        }
    }

    fn flush(&self) {
        self.term.flush();
        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                f.flush();
            }
        }
    }
}

pub fn format_line(time: &str, level: log::Level, target: &str, msg: &str) -> String {
    format!("{time} {level:<5} {target}: {msg}")
}

/// Install the logger (terminal + file). Safe to call once at startup.
pub fn init() {
    let spec = std::env::var("MANTA_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| DEFAULT_FILTER.to_string());
    let term = env_logger::Builder::new().parse_filters(&spec).build();
    let max = term.filter();
    let path = log_path();
    let file = path
        .as_deref()
        .and_then(|p| match RotatingFile::open(p, MAX_LOG_BYTES) {
            Ok(f) => Some(Mutex::new(f)),
            Err(e) => {
                eprintln!("manta: can't open log file {}: {e}", p.display());
                None
            }
        });
    let has_file = file.is_some();
    if log::set_boxed_logger(Box::new(CombinedLogger { term, file })).is_ok() {
        log::set_max_level(max);
    }
    log::info!(
        "Manta {} starting (pid {}, log file: {})",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        match (&path, has_file) {
            (Some(p), true) => p.display().to_string(),
            _ => "none".into(),
        }
    );
}

/// Show the log file's folder in Finder itself (or open it elsewhere).
///
/// Explicitly Finder: `open -R` and `NSWorkspace activateFileViewerSelectingURLs`
/// go to the default file viewer (`NSFileViewer`), which may be Manta. Finder
/// opening the folder by bundle id needs no Apple Events permission; the
/// file itself isn't selected then, but it is the folder's main item.
pub fn reveal_log_file() -> Result<PathBuf, String> {
    let path = log_path().ok_or(t!("logging.no_log_dir"))?;
    if !path.exists() {
        // Create it so there's something to show.
        RotatingFile::open(&path, MAX_LOG_BYTES).map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        let dir = path.parent().unwrap_or(&path);
        std::process::Command::new("/usr/bin/open")
            .args(finder_open_args(dir))
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let dir = path.parent().unwrap_or(&path).to_path_buf();
        open::that_detached(&dir).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

/// `open` arguments that show `dir` in Finder, whatever the default file
/// viewer is.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn finder_open_args(dir: &std::path::Path) -> Vec<std::ffi::OsString> {
    vec![
        "-b".into(),
        crate::default_viewer::FINDER_ID.into(),
        dir.as_os_str().to_owned(),
    ]
}

/// User-facing text for an error opening `name`, precise about the two
/// kinds of "permission" failures on macOS.
pub fn describe_open_error(name: &str, err: &std::io::Error) -> String {
    match err.raw_os_error() {
        Some(1) => t!("logging.blocked", name = name),
        Some(13) => t!("logging.no_read_permission", name = name),
        _ if err.kind() == std::io::ErrorKind::NotFound => t!("logging.gone", name = name),
        _ => err.to_string(),
    }
}

/// Open a local file with its default app. Checks that the file can be
/// read first so permission problems are reported exactly, and logs the raw
/// error (path, errno, backend). Returns the user-facing message.
pub fn open_local(path: &Path) -> Result<(), String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    if path.is_file() {
        if let Err(err) = File::open(path) {
            log::warn!(
                "open: can't read {} (backend local): {err:?} (os error {:?})",
                path.display(),
                err.raw_os_error()
            );
            return Err(describe_open_error(&name, &err));
        }
    }
    log::debug!("open: launching default app for {}", path.display());
    // macOS: run `open` ourselves so LaunchServices' own error text (it
    // reports e.g. "-54 permission" failures on stderr, after spawning
    // succeeded) lands in the log. A thread reaps the child: no zombies.
    #[cfg(target_os = "macos")]
    {
        use std::process::{Command, Stdio};
        let child = Command::new("/usr/bin/open")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        match child {
            Ok(child) => {
                let shown = path.display().to_string();
                std::thread::spawn(move || match child.wait_with_output() {
                    Ok(out) if !out.status.success() => log::warn!(
                        "open: `open {shown}` (backend local) failed with {}: {}",
                        out.status,
                        String::from_utf8_lossy(&out.stderr).trim()
                    ),
                    Ok(_) => log::debug!("open: `open {shown}` succeeded"),
                    Err(e) => log::warn!("open: waiting for `open {shown}` failed: {e:?}"),
                });
                return Ok(());
            }
            Err(err) => log::warn!("open: can't run /usr/bin/open: {err:?}; trying the open crate"),
        }
    }
    open::that_detached(path).map_err(|err| {
        log::warn!(
            "open: `open` failed for {} (backend local): {err:?} (os error {:?})",
            path.display(),
            err.raw_os_error()
        );
        describe_open_error(&name, &err)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Never the default file viewer (which may be Manta): Finder by id.
    #[test]
    fn log_folder_opens_in_finder_by_bundle_id() {
        let args = finder_open_args(std::path::Path::new("/Users/me/Library/Logs/Manta"));
        assert_eq!(
            args,
            ["-b", "com.apple.finder", "/Users/me/Library/Logs/Manta"]
        );
        assert!(!args.iter().any(|a| a == "-R"), "-R goes to NSFileViewer");
    }

    #[test]
    fn rotates_at_limit_and_keeps_one_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("manta.log");
        let mut f = RotatingFile::open(&path, 100).unwrap();
        let line = "x".repeat(49); // 50 bytes with the newline
        f.write_line(&line).unwrap();
        f.write_line(&line).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 100);
        assert!(!f.backup_path().exists());
        // A third line would exceed 100 bytes: rotate first.
        f.write_line("third").unwrap();
        let backup = f.backup_path();
        assert_eq!(backup, dir.path().join("logs").join("manta.log.1"));
        assert_eq!(std::fs::metadata(&backup).unwrap().len(), 100);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "third\n");
        // Rotate again: the old backup is replaced, never more than one.
        for _ in 0..3 {
            f.write_line(&line).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            format!("third\n{line}\n")
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{line}\n{line}\n")
        );
        let files = std::fs::read_dir(dir.path().join("logs")).unwrap().count();
        assert_eq!(files, 2);
    }

    #[test]
    fn reopening_continues_size_accounting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manta.log");
        std::fs::write(&path, "y".repeat(95)).unwrap();
        let mut f = RotatingFile::open(&path, 100).unwrap();
        f.write_line("0123456789").unwrap();
        assert!(f.backup_path().exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "0123456789\n");
    }

    #[test]
    fn oversized_line_in_empty_file_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manta.log");
        let mut f = RotatingFile::open(&path, 10).unwrap();
        f.write_line("a long line that exceeds the limit").unwrap();
        assert!(!f.backup_path().exists());
    }

    #[test]
    fn open_errors_are_precise() {
        let eperm = std::io::Error::from_raw_os_error(1);
        assert!(describe_open_error("a.png", &eperm)
            .starts_with("macOS blocked access to “a.png” (Operation not permitted)"));
        let eacces = std::io::Error::from_raw_os_error(13);
        assert_eq!(
            describe_open_error("a.png", &eacces),
            "You don't have read permission for “a.png”."
        );
        let missing = std::io::Error::from_raw_os_error(2);
        assert_eq!(
            describe_open_error("a.png", &missing),
            "“a.png” no longer exists."
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_file_is_reported_before_launching() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.png");
        std::fs::write(&path, b"x").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if File::open(&path).is_ok() {
            return; // running as root
        }
        let err = open_local(&path).unwrap_err();
        assert_eq!(err, "You don't have read permission for “secret.png”.");
    }

    #[test]
    fn line_format_and_path() {
        let l = format_line(
            "2026-01-01 10:00:00.000",
            log::Level::Warn,
            "manta::app",
            "boom",
        );
        assert_eq!(l, "2026-01-01 10:00:00.000 WARN  manta::app: boom");
        #[cfg(target_os = "macos")]
        assert!(log_path()
            .unwrap()
            .ends_with("Library/Logs/Manta/manta.log"));
    }
}
