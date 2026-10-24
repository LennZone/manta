//! Crash reports without tracking: when Manta dies from a panic or a fatal
//! signal (SIGSEGV, SIGBUS, SIGILL, SIGFPE, SIGABRT) it writes
//! `~/Library/Logs/Manta/crash-<UTC date>.txt` with the version, macOS
//! version, architecture, the message and a backtrace, and nothing else.
//! On the next launch a small card asks "Manta quit unexpectedly last time"
//! with Report on GitHub (the bug form, prefilled with a short summary; the
//! user attaches the file), Show Report and Ignore. Nothing is ever sent by
//! Manta itself.
//!
//! Personal data is scrubbed before anything is written: the home folder
//! becomes `~`, other `/Users/<name>` folders `/Users/<user>`, `user@host`
//! and `scheme://host` lose their names, and the hosts and user names of
//! the connections opened in this session (see [`note_sensitive`]) become
//! `<redacted>`.
//!
//! Release builds are stripped, so the backtrace lists raw frames: address,
//! image and offset, plus the main binary's UUID (`LC_UUID`) and load
//! address. With the release's dSYM (`scripts/build-release.sh` keeps one in
//! `dist/` when `KEEP_DSYM=1`; it is never shipped) a frame is symbolicated
//! with `atos`, as the report itself explains.
//!
//! A panic doesn't always end the app: workers, plugins and the aux windows
//! catch them. A panic report is therefore provisional until the next
//! frame: when the main loop draws again ([`survived`]), the app went on
//! and the report is deleted again.

use std::ffi::CStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use eframe::egui;

use crate::feedback::SystemInfo;

/// File name prefix of the reports (`crash-2026-10-24-120501.txt`).
const PREFIX: &str = "crash-";
/// Remembers the newest report the user was told about.
const SEEN_FILE: &str = ".crash-seen";
/// Keep at most this many reports.
const KEEP: usize = 10;
/// Frames recorded at most.
const MAX_FRAMES: usize = 64;
/// Frames in the GitHub summary.
const SUMMARY_FRAMES: usize = 8;

/// Where crash reports go: next to the log file.
pub fn dir() -> Option<PathBuf> {
    crate::logging::log_path().and_then(|p| p.parent().map(Path::to_path_buf))
}

// ---------------------------------------------------------------------------
// Scrubbing

/// Host and user names of connections in this session, replaced by
/// `<redacted>` in reports.
static SENSITIVE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Remember a host name, user name or connection label to redact from
/// crash reports (connections call this when they connect).
pub fn note_sensitive(s: &str) {
    let s = s.trim();
    // Very short strings ("a", "22") would redact unrelated text.
    if s.chars().count() < 3 {
        return;
    }
    if let Ok(mut v) = SENSITIVE.lock() {
        if !v.iter().any(|x| x == s) {
            v.push(s.to_string());
            // Longest first, so "deploy@web-01" goes before "web-01".
            v.sort_by_key(|x| std::cmp::Reverse(x.len()));
        }
    }
}

fn sensitive() -> Vec<String> {
    // try_lock: the panic may have happened while it was held.
    SENSITIVE.try_lock().map(|v| v.clone()).unwrap_or_default()
}

/// Remove personal data from `text`: `home` → `~`, the `sensitive` strings
/// → `<redacted>`, other users' home folders, `user@host` and the host of
/// `scheme://host`.
pub fn scrub(text: &str, home: Option<&str>, sensitive: &[String]) -> String {
    let mut s = text.to_string();
    for x in sensitive {
        if !x.is_empty() {
            s = s.replace(x.as_str(), "<redacted>");
        }
    }
    if let Some(home) = home.map(|h| h.trim_end_matches('/')) {
        if home.len() > 1 {
            s = s.replace(home, "~");
        }
    }
    let s = scrub_users_dirs(&s);
    let s = scrub_urls(&s);
    scrub_user_at_host(&s)
}

/// `/Users/name/…` → `/Users/<user>/…` (`/Users/Shared` stays).
fn scrub_users_dirs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("/Users/") {
        out.push_str(&rest[..i + 7]);
        rest = &rest[i + 7..];
        let end = rest
            .find(|c: char| c == '/' || c.is_whitespace() || c == '"' || c == '\'')
            .unwrap_or(rest.len());
        let name = &rest[..end];
        if name.is_empty() || name == "Shared" || name == "<user>" {
            out.push_str(name);
        } else {
            out.push_str("<user>");
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn is_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')
}

/// `sftp://anna@files.example.com:22/x` → `sftp://<host>:22/x`.
fn scrub_urls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("://") {
        out.push_str(&rest[..i + 3]);
        rest = &rest[i + 3..];
        let end = rest
            .find(|c: char| !(is_host_char(c) || c == '@' || c == '<' || c == '>'))
            .unwrap_or(rest.len());
        if end > 0 {
            out.push_str("<host>");
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// `deploy@web-01.example.com` → `<user>@<host>`.
fn scrub_user_at_host(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            // The user part already written: take it back.
            let mut start = out.len();
            for (pos, c) in out.char_indices().rev() {
                if is_host_char(c) {
                    start = pos;
                } else {
                    break;
                }
            }
            let mut end = i + 1;
            // An already redacted host (`deploy@<redacted>`) keeps its mark.
            let marker = chars.get(end) == Some(&'<');
            if marker {
                while end < chars.len() && chars[end] != '>' {
                    end += 1;
                }
                end = (end + 1).min(chars.len());
            } else {
                while end < chars.len() && is_host_char(chars[end]) {
                    end += 1;
                }
            }
            let user = &out[start..];
            if !user.is_empty() && end > i + 1 {
                out.truncate(start);
                out.push_str("<user>@");
                if marker {
                    out.extend(&chars[i + 1..end]);
                } else {
                    out.push_str("<host>");
                }
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Reports

/// One backtrace frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub addr: usize,
    /// File name of the image (`manta`, `libsystem_kernel.dylib`).
    pub image: Option<String>,
    /// Offset of `addr` in the image.
    pub offset: Option<usize>,
    /// The symbol, if the image has one (not in stripped release builds).
    pub symbol: Option<String>,
}

/// The main binary, to symbolicate with its dSYM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub name: String,
    /// `LC_UUID`, e.g. `2F9C…-…`.
    pub uuid: Option<String>,
    /// Load address (the Mach-O header in memory).
    pub base: usize,
    /// Size of `__TEXT`, to tell its frames from the system's.
    pub text_size: usize,
}

/// What a report says (everything but the system details).
pub struct Crash<'a> {
    /// "panic" or "signal SIGSEGV".
    pub kind: &'a str,
    pub thread: &'a str,
    pub message: &'a str,
    pub location: Option<&'a str>,
    pub frames: &'a [Frame],
}

/// `atos` arch name for this build.
fn atos_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    }
}

/// The report text (already scrubbed by the caller where needed).
pub fn report_text(info: &SystemInfo, image: Option<&Image>, when: i64, c: &Crash) -> String {
    let mut s = String::new();
    s.push_str("Manta crash report\n==================\n\n");
    s.push_str(&format!("Manta: {}\n", info.manta));
    s.push_str(&format!("macOS: {}\n", info.macos));
    s.push_str(&format!("Architecture: {}\n", info.arch));
    s.push_str(&format!("Time: {} UTC\n", utc_stamp(when, true)));
    s.push_str(&format!("Kind: {}\n", c.kind));
    s.push_str(&format!("Thread: {}\n", c.thread));
    s.push_str(&format!("Message: {}\n", c.message));
    if let Some(l) = c.location {
        s.push_str(&format!("Location: {l}\n"));
    }
    if let Some(img) = image {
        s.push_str(&format!(
            "\nBinary: {}\nUUID: {}\nLoad address: {:#x}\n",
            img.name,
            img.uuid.as_deref().unwrap_or("unknown"),
            img.base
        ));
    }
    s.push_str("\nBacktrace:\n");
    for (i, f) in c.frames.iter().enumerate() {
        s.push_str(&format_frame(i, f));
        s.push('\n');
    }
    if let Some(img) = image {
        let own: Vec<String> = c
            .frames
            .iter()
            .filter(|f| f.image.as_deref() == Some(img.name.as_str()))
            .map(|f| format!("{:#x}", f.addr))
            .collect();
        if !own.is_empty() {
            s.push_str(&format!(
                "\nSymbolicate the {} frames with the dSYM of this release (same UUID):\n  \
                 atos -arch {} -o {}.dSYM/Contents/Resources/DWARF/{} -l {:#x} {}\n",
                img.name,
                atos_arch(),
                img.name,
                img.name,
                img.base,
                own.join(" ")
            ));
        }
    }
    s
}

fn format_frame(i: usize, f: &Frame) -> String {
    let mut line = format!("{i:>3}  {:#018x}", f.addr);
    if let Some(img) = &f.image {
        line.push_str(&format!("  {img}"));
        if let Some(off) = f.offset {
            line.push_str(&format!(" + {off:#x}"));
        }
    }
    if let Some(sym) = &f.symbol {
        line.push_str(&format!("  {sym}"));
    }
    line
}

/// `2026-10-24-120501` (file names) or `2026-10-24 12:05:01` for `secs`
/// since 1970, in UTC.
pub fn utc_stamp(secs: i64, human: bool) -> String {
    let mut buf = [0u8; 19];
    let n = write_stamp(&mut buf, secs, human);
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// [`utc_stamp`] into `buf` without allocating (signal handler).
fn write_stamp(buf: &mut [u8; 19], secs: i64, human: bool) -> usize {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (hh, mm, ss) = (rem / 3600, rem / 60 % 60, rem % 60);
    let mut n = 0;
    put_num(buf, &mut n, y, 4);
    put_byte(buf, &mut n, b'-');
    put_num(buf, &mut n, m, 2);
    put_byte(buf, &mut n, b'-');
    put_num(buf, &mut n, d, 2);
    put_byte(buf, &mut n, if human { b' ' } else { b'-' });
    put_num(buf, &mut n, hh, 2);
    if human {
        put_byte(buf, &mut n, b':');
    }
    put_num(buf, &mut n, mm, 2);
    if human {
        put_byte(buf, &mut n, b':');
    }
    put_num(buf, &mut n, ss, 2);
    n
}

fn put_byte(buf: &mut [u8; 19], n: &mut usize, b: u8) {
    buf[*n] = b;
    *n += 1;
}

fn put_num(buf: &mut [u8; 19], n: &mut usize, v: i64, width: usize) {
    let mut v = v.max(0);
    for k in (0..width).rev() {
        buf[*n + k] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    *n += width;
}

/// Write `text` as a new report in `dir`; returns its path.
pub fn write_report(dir: &Path, when: i64, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = utc_stamp(when, false);
    for n in 1.. {
        let name = if n == 1 {
            format!("{PREFIX}{stamp}.txt")
        } else {
            format!("{PREFIX}{stamp}-{n}.txt")
        };
        let path = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(text.as_bytes())?;
                prune(dir);
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 100 => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// The reports in `dir`, oldest first (the names sort by time).
fn reports(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(".txt"))
        })
        .collect();
    // By name without ".txt": "crash-…01" before "crash-…01-2".
    v.sort_by_key(|p| stem(p));
    v
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Delete all but the newest [`KEEP`] reports.
fn prune(dir: &Path) {
    let all = reports(dir);
    if all.len() > KEEP {
        for p in &all[..all.len() - KEEP] {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// The newest report the user hasn't been told about yet.
pub fn pending(dir: &Path) -> Option<PathBuf> {
    let newest = reports(dir).pop()?;
    let seen = std::fs::read_to_string(dir.join(SEEN_FILE)).unwrap_or_default();
    (stem(&newest).as_str() > seen.trim()).then_some(newest)
}

/// Don't ask about `report` (and older ones) again.
pub fn mark_seen(dir: &Path, report: &Path) {
    let _ = std::fs::write(dir.join(SEEN_FILE), stem(report));
}

/// The short summary for the GitHub form: the first lines of the report
/// (kind, message, location) and the first frames.
pub fn summary(report: &str) -> String {
    let mut out = Vec::new();
    for key in ["Kind:", "Message:", "Location:", "UUID:"] {
        if let Some(l) = report.lines().find(|l| l.starts_with(key)) {
            out.push(l.to_string());
        }
    }
    let frames: Vec<&str> = report
        .lines()
        .skip_while(|l| *l != "Backtrace:")
        .skip(1)
        .take_while(|l| !l.is_empty())
        .take(SUMMARY_FRAMES)
        .collect();
    if !frames.is_empty() {
        out.push("First frames:".into());
        out.extend(frames.iter().map(|f| f.to_string()));
    }
    out.join("\n")
}

/// The bug form for `report`: version fields plus the summary; the user
/// attaches the full report (URLs can't carry files).
pub fn issue_url(info: &SystemInfo, report_name: &str, report: &str) -> String {
    let first = report
        .lines()
        .find_map(|l| l.strip_prefix("Message: "))
        .unwrap_or("crash");
    let title: String = format!("Crash: {first}").chars().take(100).collect();
    let summary = summary(report);
    let actual = |summary: &str| {
        format!(
            "Manta quit unexpectedly.\n\n```\n{summary}\n```\n\n\
             Full report: `~/Library/Logs/Manta/{report_name}` (please attach it here)."
        )
    };
    crate::feedback::crash_issue_url(info, &title, &actual(&summary), |budget| {
        // Too long: shorten the summary until it fits.
        let mut s: String = summary.chars().take(budget).collect();
        if s.len() < summary.len() {
            s.push_str("\n…");
        }
        actual(&s)
    })
}

// ---------------------------------------------------------------------------
// The notice at the next launch

/// "Manta quit unexpectedly last time" for one report.
pub struct Notice {
    pub path: PathBuf,
    report: String,
}

impl Notice {
    /// The newest report not shown yet, marked as shown right away (the
    /// notice appears once, whatever the user does with it).
    pub fn take(dir: &Path) -> Option<Self> {
        let path = pending(dir)?;
        mark_seen(dir, &path);
        let report = std::fs::read_to_string(&path).ok()?;
        log::info!("crash: report from an earlier run: {}", path.display());
        Some(Self { path, report })
    }

    /// The GitHub bug form, prefilled with a summary of the report.
    pub fn issue_url(&self) -> String {
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        issue_url(&SystemInfo::current(), &name, &self.report)
    }
}

/// A button of the notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Report,
    Show,
    Ignore,
}

/// Draw the notice as a card with its bottom right corner at
/// `bottom_right` (where the toasts start); returns the clicked button and
/// the card's top edge (the toasts go above it).
pub fn notice_ui(
    ctx: &egui::Context,
    _notice: &Notice,
    bottom_right: egui::Pos2,
) -> (Option<Choice>, f32) {
    use crate::theme::{self, radius, space, text};
    use crate::widgets::{self, ButtonKind};
    let p = theme::pal_ctx(ctx);
    let width = 440.0_f32.min(ctx.content_rect().width() - 2.0 * space::XL);
    let mut choice = None;
    let area = egui::Area::new(egui::Id::unique("crash-notice"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .fixed_pos(bottom_right)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.elevated)
                .stroke(egui::Stroke::new(1.0, p.separator))
                .corner_radius(egui::CornerRadius::same(radius::PANEL + 2))
                .shadow(ui.visuals().popup_shadow)
                .inner_margin(egui::Margin::symmetric(
                    space::LG as i8,
                    space::MD as i8 + 2,
                ))
                .show(ui, |ui| {
                    ui.set_width(width - 2.0 * space::LG);
                    ui.horizontal_top(|ui| {
                        widgets::icon(ui, crate::icons::WARNING, 18.0, p.warning);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = space::XXS;
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(t!("crash.title"))
                                        .font(theme::medium(text::BODY))
                                        .color(p.text),
                                )
                                .wrap(),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(t!("crash.detail")).color(p.text_secondary),
                                )
                                .wrap(),
                            );
                            ui.add_space(space::SM);
                            ui.horizontal_wrapped(|ui| {
                                let buttons = [
                                    (Choice::Report, t!("crash.report"), ButtonKind::Primary),
                                    (Choice::Show, t!("crash.show"), ButtonKind::Secondary),
                                    (Choice::Ignore, t!("crash.ignore"), ButtonKind::Secondary),
                                ];
                                for (c, label, kind) in buttons {
                                    if widgets::button(ui, label, kind, true).clicked() {
                                        choice = Some(c);
                                    }
                                }
                            });
                        });
                    });
                });
        });
    (choice, area.response.rect.top())
}

// ---------------------------------------------------------------------------
// Capturing

/// Raw return addresses of the current stack.
fn capture_addrs(buf: &mut [*mut libc::c_void; MAX_FRAMES]) -> usize {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: `buf` holds MAX_FRAMES pointers.
        let n = unsafe { libc::backtrace(buf.as_mut_ptr(), MAX_FRAMES as libc::c_int) };
        n.max(0) as usize
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = buf;
        0
    }
}

/// Image, offset and symbol of each address (not signal-safe).
fn resolve(addrs: &[*mut libc::c_void], home: Option<&str>, sens: &[String]) -> Vec<Frame> {
    addrs
        .iter()
        .map(|&a| {
            let mut f = Frame {
                addr: a as usize,
                image: None,
                offset: None,
                symbol: None,
            };
            #[cfg(target_os = "macos")]
            {
                let mut info = libc::Dl_info {
                    dli_fname: std::ptr::null(),
                    dli_fbase: std::ptr::null_mut(),
                    dli_sname: std::ptr::null(),
                    dli_saddr: std::ptr::null_mut(),
                };
                // SAFETY: dladdr only reads the loader's tables.
                if unsafe { libc::dladdr(a as *const libc::c_void, &mut info) } != 0 {
                    if !info.dli_fname.is_null() {
                        // SAFETY: a C string owned by dyld.
                        let path = unsafe { CStr::from_ptr(info.dli_fname) }.to_string_lossy();
                        f.image = Some(
                            Path::new(path.as_ref())
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| scrub(&path, home, sens)),
                        );
                        f.offset = Some((a as usize).wrapping_sub(info.dli_fbase as usize));
                    }
                    if !info.dli_sname.is_null() {
                        // SAFETY: as above.
                        let sym = unsafe { CStr::from_ptr(info.dli_sname) }.to_string_lossy();
                        f.symbol = Some(scrub(&sym, home, sens));
                    }
                }
            }
            f
        })
        .collect()
}

/// The main executable's name, UUID, load address and `__TEXT` size.
fn main_image() -> Option<Image> {
    #[cfg(target_os = "macos")]
    {
        // The image this function is in: the executable (Manta links its
        // Rust code statically). Its base is the mapped Mach-O header.
        let mut info = libc::Dl_info {
            dli_fname: std::ptr::null(),
            dli_fbase: std::ptr::null_mut(),
            dli_sname: std::ptr::null(),
            dli_saddr: std::ptr::null_mut(),
        };
        let here = main_image as *const () as *const libc::c_void;
        // SAFETY: dladdr only reads the loader's tables.
        if unsafe { libc::dladdr(here, &mut info) } == 0 || info.dli_fbase.is_null() {
            return None;
        }
        let header = info.dli_fbase as *const u8;
        // SAFETY: a valid Mach-O header (64-bit on every Mac Manta runs on).
        let (uuid, text_size) = unsafe { parse_load_commands(header) };
        let name = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "manta".into());
        Some(Image {
            name,
            uuid,
            base: header as usize,
            text_size,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// `LC_UUID` and the `__TEXT` segment size from a mapped Mach-O header.
///
/// # Safety
/// `header` points to a mapped 64-bit Mach-O header with its load commands.
#[cfg(target_os = "macos")]
unsafe fn parse_load_commands(header: *const u8) -> (Option<String>, usize) {
    const MH_MAGIC_64: u32 = 0xfeed_facf;
    const LC_SEGMENT_64: u32 = 0x19;
    const LC_UUID: u32 = 0x1b;
    // SAFETY (for every `rd`): the caller vouches for the load commands.
    let rd = |p: *const u8| unsafe { (p as *const u32).read_unaligned() };
    if rd(header) != MH_MAGIC_64 {
        return (None, 0);
    }
    let ncmds = rd(unsafe { header.add(16) });
    let mut p = unsafe { header.add(32) };
    let (mut uuid, mut text) = (None, 0usize);
    for _ in 0..ncmds {
        let cmd = rd(p);
        let size = rd(unsafe { p.add(4) }) as usize;
        if cmd == LC_UUID {
            let b = unsafe { std::slice::from_raw_parts(p.add(8), 16) };
            uuid = Some(format_uuid(b));
        } else if cmd == LC_SEGMENT_64 {
            let name = unsafe { std::slice::from_raw_parts(p.add(8), 16) };
            if name.starts_with(b"__TEXT\0") {
                text = unsafe { (p.add(32) as *const u64).read_unaligned() } as usize;
            }
        }
        if size == 0 {
            break;
        }
        p = unsafe { p.add(size) };
    }
    (uuid, text)
}

/// `2F9C3A1E-…` like `dwarfdump --uuid` prints it.
pub fn format_uuid(b: &[u8]) -> String {
    let hex: Vec<String> = b.iter().map(|x| format!("{x:02X}")).collect();
    let h = hex.concat();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

// ---------------------------------------------------------------------------
// Installing

struct Config {
    dir: PathBuf,
    home: Option<String>,
    info: SystemInfo,
    image: Option<Image>,
    /// Signal reports: the path prefix (`<dir>/crash-`) and the fixed
    /// header, prepared here because the handler can't allocate.
    sig_prefix: Vec<u8>,
    sig_header: Vec<u8>,
    /// The "Binary / UUID / Load address" block.
    sig_binary: Vec<u8>,
}

static CONFIG: OnceLock<Config> = OnceLock::new();
/// Reports of panics in this session; deleted when the app goes on.
static PROVISIONAL: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// The thread of the last panic (`pthread_self`), so the abort that may
/// follow it doesn't write a second report.
static PANIC_THREAD: AtomicUsize = AtomicUsize::new(0);

/// Install the panic hook and the fatal-signal handlers, writing reports
/// to `dir` (normally [`dir`]). Call once at startup, after logging.
pub fn install(dir: PathBuf) {
    let home = dirs::home_dir().map(|h| h.to_string_lossy().into_owned());
    let info = SystemInfo::current();
    let image = main_image();
    let mut sig_prefix = dir.as_os_str().as_encoded_bytes().to_vec();
    sig_prefix.extend_from_slice(b"/");
    sig_prefix.extend_from_slice(PREFIX.as_bytes());
    let header = report_text(
        &info,
        image.as_ref(),
        0,
        &Crash {
            kind: "",
            thread: "",
            message: "",
            location: None,
            frames: &[],
        },
    );
    // Everything up to "Time:"; the handler writes the rest itself.
    let time = header.find("Time:").unwrap_or(header.len());
    let sig_header = header[..time].to_string();
    let sig_binary = match (header.find("\nBinary:"), header.find("\nBacktrace:")) {
        (Some(a), Some(b)) if a < b => header[a..b].to_string(),
        _ => String::new(),
    };
    let _ = std::fs::create_dir_all(&dir);
    if CONFIG
        .set(Config {
            dir,
            home,
            info,
            image,
            sig_prefix,
            sig_header: sig_header.into_bytes(),
            sig_binary: sig_binary.into_bytes(),
        })
        .is_err()
    {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |p| {
        on_panic(p);
        previous(p);
    }));
    install_signal_handlers();
}

fn on_panic(p: &std::panic::PanicHookInfo<'_>) {
    let Some(cfg) = CONFIG.get() else {
        return;
    };
    let sens = sensitive();
    let home = cfg.home.as_deref();
    let message = p
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(non-string panic payload)".into());
    // A panic that can't unwind (e.g. out of a winit callback) panics
    // again with this message right before aborting: one report is enough.
    #[cfg(unix)]
    // SAFETY: always safe.
    let me = unsafe { libc::pthread_self() } as usize;
    #[cfg(unix)]
    if message.starts_with("panic in a function that cannot unwind")
        && PANIC_THREAD.load(Ordering::SeqCst) == me
    {
        return;
    }
    #[cfg(unix)]
    PANIC_THREAD.store(me, Ordering::SeqCst);
    let location = p.location().map(|l| {
        scrub(
            &format!("{}:{}:{}", l.file(), l.line(), l.column()),
            home,
            &sens,
        )
    });
    let thread = std::thread::current()
        .name()
        .map(|n| scrub(n, home, &sens))
        .unwrap_or_else(|| "(unnamed)".into());
    let mut addrs = [std::ptr::null_mut(); MAX_FRAMES];
    let n = capture_addrs(&mut addrs);
    let frames = resolve(&addrs[..n], home, &sens);
    let text = report_text(
        &cfg.info,
        cfg.image.as_ref(),
        now_secs(),
        &Crash {
            kind: "panic",
            thread: &thread,
            message: &scrub(&message, home, &sens),
            location: location.as_deref(),
            frames: &frames,
        },
    );
    match write_report(&cfg.dir, now_secs(), &text) {
        Ok(path) => {
            if let Ok(mut v) = PROVISIONAL.try_lock() {
                v.push(path);
            }
        }
        Err(e) => eprintln!("manta: can't write the crash report: {e}"),
    }
}

/// The main loop drew another frame: the panics reported so far didn't
/// end the app (caught by a worker, plugin or window), so their reports
/// go again.
pub fn survived() {
    let Ok(mut v) = PROVISIONAL.try_lock() else {
        return;
    };
    for p in v.drain(..) {
        log::debug!(
            "crash: the app went on after a panic, removing {}",
            p.display()
        );
        let _ = std::fs::remove_file(p);
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
const SIGNALS: [(libc::c_int, &str); 5] = [
    (libc::SIGSEGV, "SIGSEGV"),
    (libc::SIGBUS, "SIGBUS"),
    (libc::SIGILL, "SIGILL"),
    (libc::SIGFPE, "SIGFPE"),
    (libc::SIGABRT, "SIGABRT"),
];

/// The handlers that were installed before ours (Rust's stack overflow
/// handler for SIGSEGV / SIGBUS), restored before the signal goes on.
#[cfg(target_os = "macos")]
static mut PREVIOUS: [std::mem::MaybeUninit<libc::sigaction>; 5] =
    [const { std::mem::MaybeUninit::uninit() }; 5];

fn install_signal_handlers() {
    #[cfg(target_os = "macos")]
    {
        // Load the unwinder now: its first use may allocate.
        let mut warm = [std::ptr::null_mut(); MAX_FRAMES];
        capture_addrs(&mut warm);
        for (i, (sig, _)) in SIGNALS.iter().enumerate() {
            // SAFETY: plain sigaction calls; PREVIOUS is written here, once,
            // before the handler can run.
            unsafe {
                let mut act: libc::sigaction = std::mem::zeroed();
                act.sa_sigaction = on_signal as *const () as usize;
                act.sa_flags = libc::SA_ONSTACK;
                libc::sigemptyset(&mut act.sa_mask);
                let prev = &raw mut PREVIOUS;
                libc::sigaction(*sig, &act, (*prev)[i].as_mut_ptr());
            }
        }
    }
}

/// A fixed buffer the signal handler formats into (no allocation).
#[cfg(target_os = "macos")]
struct SigBuf {
    data: [u8; 8192],
    len: usize,
}

#[cfg(target_os = "macos")]
impl SigBuf {
    fn push(&mut self, b: &[u8]) {
        let n = b.len().min(self.data.len() - self.len);
        self.data[self.len..self.len + n].copy_from_slice(&b[..n]);
        self.len += n;
    }
    fn hex(&mut self, v: usize) {
        let mut tmp = [0u8; 18];
        tmp[0] = b'0';
        tmp[1] = b'x';
        let mut n = 2;
        let mut started = false;
        for shift in (0..16).rev() {
            let d = (v >> (shift * 4)) & 0xf;
            if d != 0 || started || shift == 0 {
                started = true;
                tmp[n] = b"0123456789abcdef"[d];
                n += 1;
            }
        }
        self.push(&tmp[..n]);
    }
    /// 16 hex digits, no prefix.
    fn hex16(&mut self, v: usize) {
        let mut tmp = [0u8; 16];
        for (k, t) in tmp.iter_mut().enumerate() {
            *t = b"0123456789abcdef"[(v >> ((15 - k) * 4)) & 0xf];
        }
        self.push(&tmp);
    }
    fn dec(&mut self, v: usize) {
        let mut tmp = [0u8; 20];
        let mut n = tmp.len();
        let mut v = v;
        loop {
            n -= 1;
            tmp[n] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        self.push(&tmp[n..]);
    }
}

/// Write a minimal report with only async-signal-safe calls, then let the
/// signal go on to the previous handler (or the default: the process ends).
#[cfg(target_os = "macos")]
extern "C" fn on_signal(sig: libc::c_int) {
    let idx = SIGNALS.iter().position(|(s, _)| *s == sig);
    // SAFETY: async-signal-safe calls only (time, open, write, close,
    // backtrace after the warm-up, sigaction, raise, pthread_self).
    unsafe {
        let after_panic = sig == libc::SIGABRT
            && PANIC_THREAD.load(Ordering::SeqCst) == libc::pthread_self() as usize;
        if let (Some(cfg), Some(i), false) = (CONFIG.get(), idx, after_panic) {
            write_signal_report(cfg, SIGNALS[i].1);
        }
        if let Some(i) = idx {
            let prev = &raw const PREVIOUS;
            libc::sigaction(sig, (*prev)[i].as_ptr(), std::ptr::null_mut());
        }
        // A fault repeats when the handler returns and then reaches the
        // previous handler; an abort has to be raised again.
        if sig == libc::SIGABRT {
            libc::raise(sig);
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn write_signal_report(cfg: &Config, name: &str) {
    let secs = unsafe { libc::time(std::ptr::null_mut()) } as i64;
    let mut stamp = [0u8; 19];
    let mut path = SigBuf {
        data: [0; 8192],
        len: 0,
    };
    path.push(&cfg.sig_prefix);
    let n = write_stamp(&mut stamp, secs, false);
    path.push(&stamp[..n]);
    path.push(b"-signal.txt\0");
    let fd = unsafe {
        libc::open(
            path.data.as_ptr() as *const libc::c_char,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
    };
    if fd < 0 {
        return;
    }
    let mut b = SigBuf {
        data: [0; 8192],
        len: 0,
    };
    b.push(&cfg.sig_header);
    b.push(b"Time: ");
    let n = write_stamp(&mut stamp, secs, true);
    b.push(&stamp[..n]);
    b.push(b" UTC\nKind: signal ");
    b.push(name.as_bytes());
    b.push(b"\nThread: (signal)\nMessage: fatal signal ");
    b.push(name.as_bytes());
    b.push(b"\n");
    b.push(&cfg.sig_binary);
    let mut addrs = [std::ptr::null_mut(); MAX_FRAMES];
    let count = capture_addrs(&mut addrs);
    b.push(b"\nBacktrace:\n");
    if count == 0 {
        // Some macOS versions can't unwind from a signal handler.
        b.push(b"  (no frames available)\n");
    }
    for (i, a) in addrs[..count].iter().enumerate() {
        let a = *a as usize;
        // Like `format_frame`: "  3  0x0000000100004f20".
        b.push(if i < 10 {
            b"  "
        } else if i < 100 {
            b" "
        } else {
            b""
        });
        b.dec(i);
        b.push(b"  0x");
        b.hex16(a);
        if let Some(img) = &cfg.image {
            if a >= img.base && a < img.base + img.text_size {
                b.push(b"  ");
                b.push(img.name.as_bytes());
                b.push(b" + ");
                b.hex(a - img.base);
            }
        }
        b.push(b"\n");
    }
    if let Some(img) = &cfg.image {
        b.push(b"\nSymbolicate the ");
        b.push(img.name.as_bytes());
        b.push(b" frames with the dSYM of this release (same UUID):\n  atos -arch ");
        b.push(atos_arch().as_bytes());
        b.push(b" -o ");
        b.push(img.name.as_bytes());
        b.push(b".dSYM/Contents/Resources/DWARF/");
        b.push(img.name.as_bytes());
        b.push(b" -l ");
        b.hex(img.base);
        b.push(b" <address> ...\n");
    }
    unsafe {
        libc::write(fd, b.data.as_ptr() as *const libc::c_void, b.len);
        libc::close(fd);
    }
}

#[cfg(test)]
mod tests;
