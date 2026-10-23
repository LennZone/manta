//! The background side of Image Tools: tool discovery, the info cache, the
//! per-folder badge scan, the panel model, preparing the sheet, and jobs.
//! Everything here runs on worker threads; the plugin's trait methods only
//! post work and read [`Shared`].

use std::collections::HashMap;
use std::io::Read as _;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use manta_core::VfsPath;
use manta_plugin::{Decoration, HostRequest, PaneId, PaneState, PluginHandle, ToastLevel};
use sha2::{Digest, Sha256};

use crate::headers::{self, Exif, Info};
use crate::i18n::tr;
use crate::meta::{self, path_str};
use crate::ops::{self, Options, Step, Work};

/// Images per folder that get decorations.
pub const SCAN_LIMIT: usize = 2000;
/// Files per `sips -g` call.
const SIPS_BATCH: usize = 64;
/// px, HEIC/TIFF previews made with sips.
const PREVIEW_EDGE: u32 = 1024;
/// Cached previews kept.
const PREVIEW_KEEP: usize = 200;
/// Seconds before the progress toast.
const PROGRESS_AFTER: Duration = Duration::from_millis(800);
pub const MODAL: &str = "tools";

// ------------------------------------------------------------------ tools

/// The external tools, found once at start (`$PATH` plus the usual
/// folders, since apps started from the Finder have a short `$PATH`).
#[derive(Debug, Clone, Default)]
pub struct Tools {
    found: HashMap<&'static str, PathBuf>,
}

pub const TOOL_NAMES: [&str; 6] = ["sips", "mdls", "exiftool", "cwebp", "pbcopy", "open"];

impl Tools {
    pub fn find() -> Tools {
        Tools {
            found: TOOL_NAMES
                .iter()
                .filter_map(|n| which(n).map(|p| (*n, p)))
                .collect(),
        }
    }

    /// Only these tools (tests).
    pub fn with(found: &[(&'static str, PathBuf)]) -> Tools {
        Tools {
            found: found.iter().cloned().collect(),
        }
    }

    pub fn has(&self, name: &str) -> bool {
        self.found.contains_key(name)
    }

    pub fn path(&self, name: &str) -> Option<&PathBuf> {
        self.found.get(name)
    }
}

pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        if !dirs.iter().any(|d| d == Path::new(extra)) {
            dirs.push(extra.into());
        }
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| {
        std::fs::metadata(p).is_ok_and(|m| {
            m.is_file() && std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0
        })
    })
}

/// Run a tool and wait (at most `timeout`); (exit code, stdout, stderr).
/// Never through a shell: `argv` goes to the process as is.
pub fn run(
    tools: &Tools,
    argv: &[String],
    timeout: Duration,
    input: Option<&[u8]>,
) -> (i32, String, String) {
    run_cancellable(tools, argv, timeout, input, None)
}

fn run_cancellable(
    tools: &Tools,
    argv: &[String],
    timeout: Duration,
    input: Option<&[u8]>,
    cancel: Option<&AtomicBool>,
) -> (i32, String, String) {
    let Some((exe, args)) = argv.split_first() else {
        return (127, String::new(), String::new());
    };
    let exe = tools
        .path(exe)
        .cloned()
        .unwrap_or_else(|| PathBuf::from(exe));
    let child = Command::new(exe)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (127, String::new(), e.to_string()),
    };
    if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
        use std::io::Write as _;
        let _ = stdin.write_all(data);
    }
    let reader = |r: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut v = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut v);
            }
            String::from_utf8_lossy(&v).into_owned()
        })
    };
    let out = reader(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    );
    let err = reader(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    );
    let start = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code().unwrap_or(-1),
            Ok(None)
                if start.elapsed() > timeout
                    || cancel.is_some_and(|c| c.load(Ordering::Relaxed)) =>
            {
                let _ = child.kill();
                let _ = child.wait();
                break -9;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(_) => break -1,
        }
    };
    (
        code,
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    )
}

// ------------------------------------------------------------------ state

/// A single image's details for the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Details {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: i128,
    pub info: Option<Info>,
    pub exif: Exif,
    /// A file the panel can draw: the image itself, or a sips-made PNG.
    pub preview: Option<PathBuf>,
}

/// What the side panel shows (texts are made while drawing, so a language
/// switch needs no rebuild).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PanelModel {
    #[default]
    Waiting,
    Remote,
    NoImage,
    CantRead(String),
    Single(Box<Details>),
    /// (path, size, info) of each selected image.
    Multi(Vec<(PathBuf, u64, Option<Info>)>),
}

/// A running batch.
pub struct Job {
    pub op: String,
    pub total: usize,
    pub index: AtomicUsize,
    pub current: Mutex<Option<PathBuf>>,
    pub done: AtomicBool,
    pub cancelled: AtomicBool,
}

/// A @2x/@1x row: Ok((@2x name, @1x name, original renamed)) or
/// Err((name, why not)).
pub type RetinaRow = Result<(String, String, bool), (String, String)>;

/// The open Image Tools sheet.
#[derive(Debug, Clone)]
pub struct Modal {
    pub op: String,
    pub files: Vec<PathBuf>,
    pub opts: Options,
    /// Asking before overwriting.
    pub confirm: bool,
    /// (size, info) per file.
    pub infos: HashMap<PathBuf, (u64, Option<Info>)>,
    /// Output names of the first file: (mode, extension) → name.
    pub previews: HashMap<(String, String), String>,
    /// @2x/@1x rows for the first files: Ok((two, one, renamed)) or
    /// Err((name, why)).
    pub retina: Vec<RetinaRow>,
    pub have_cwebp: bool,
    pub have_exiftool: bool,
}

impl Modal {
    pub fn first_info(&self) -> Option<&Info> {
        self.files
            .first()
            .and_then(|f| self.infos.get(f))
            .and_then(|(_, i)| i.as_ref())
    }

    pub fn preview(&self, mode: &str, ext: &str) -> String {
        self.previews
            .get(&(mode.to_string(), ext.to_string()))
            .cloned()
            .unwrap_or_default()
    }
}

pub struct Shared {
    pub tools: Tools,
    pub tools_ready: bool,
    pub data_dir: PathBuf,
    /// path → (mtime ns, size, info).
    cache: HashMap<PathBuf, (i128, u64, Option<Info>)>,
    pub options: HashMap<String, Options>,
    pub panel: PanelModel,
    /// The active pane's selected images (context commands).
    pub images: Vec<PathBuf>,
    /// One image that can be copied as a data URI.
    pub datauri: bool,
    pub modal: Option<Modal>,
    pub job: Option<Arc<Job>>,
}

impl Shared {
    pub fn new(data_dir: PathBuf) -> Shared {
        Shared {
            tools: Tools::default(),
            tools_ready: false,
            data_dir,
            cache: HashMap::new(),
            options: HashMap::new(),
            panel: PanelModel::Waiting,
            images: Vec::new(),
            datauri: false,
            modal: None,
            job: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|j| !j.done.load(Ordering::Relaxed))
    }

    /// The remembered options of `op` (never starting in overwrite mode;
    /// WebP falls back to JPEG without cwebp).
    pub fn remembered(&self, op: &str) -> Options {
        let mut o = self
            .options
            .get(op)
            .cloned()
            .unwrap_or_else(|| Options::for_op(op));
        if o.output == "overwrite" {
            o.output = "suffix".into();
        }
        if op == "convert" && o.format == "webp" && !self.tools.has("cwebp") {
            o.format = "jpeg".into();
        }
        o
    }
}

pub type SharedRef = Arc<Mutex<Shared>>;

pub fn lock(s: &SharedRef) -> MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// Work for the engine thread.
pub enum Msg {
    /// The active pane's state changed: rebuild the panel and commands.
    Panel {
        state: Option<PaneState>,
    },
    Shutdown,
}

// ------------------------------------------------------------------ info

fn stat(p: &Path) -> Option<(i128, u64)> {
    let m = std::fs::metadata(p).ok()?;
    m.is_file().then(|| {
        (
            m.mtime() as i128 * 1_000_000_000 + m.mtime_nsec() as i128,
            m.len(),
        )
    })
}

/// {path: info} for files without a header parser (HEIC, TIFF…), via
/// batched `sips -g`.
fn sips_infos(tools: &Tools, paths: &[PathBuf]) -> HashMap<PathBuf, Info> {
    let mut out = HashMap::new();
    if !tools.has("sips") {
        return out;
    }
    for chunk in paths.chunks(SIPS_BATCH) {
        let mut argv: Vec<String> = [
            "sips",
            "-g",
            "pixelWidth",
            "-g",
            "pixelHeight",
            "-g",
            "profile",
            "-g",
            "hasAlpha",
            "-g",
            "dpiWidth",
            "-g",
            "dpiHeight",
            "-g",
            "space",
            "-g",
            "bitsPerSample",
        ]
        .map(String::from)
        .to_vec();
        argv.extend(chunk.iter().map(|p| path_str(p)));
        let (_, text, _) = run(tools, &argv, Duration::from_secs(60), None);
        let parsed = meta::parse_sips(&text);
        for p in chunk {
            let s = path_str(p);
            if let Some(i) = meta::info_from_sips(parsed.get(&s), &meta::ext_of(&s)) {
                out.insert(p.clone(), i);
            }
        }
    }
    out
}

/// {path: (size, info)} using the mtime cache; `deep` re-reads (frame
/// counts). Missing files are left out.
pub fn infos_for(
    shared: &SharedRef,
    paths: &[PathBuf],
    deep: bool,
) -> HashMap<PathBuf, (u64, Option<Info>)> {
    let mut result = HashMap::new();
    let mut need_sips = Vec::new();
    let tools = lock(shared).tools.clone();
    for p in paths {
        let Some((mtime, size)) = stat(p) else {
            continue;
        };
        if !deep {
            if let Some((m, _, info)) = lock(shared).cache.get(p) {
                if *m == mtime {
                    result.insert(p.clone(), (size, info.clone()));
                    continue;
                }
            }
        }
        let ext = meta::ext_of(&path_str(p));
        if meta::HEADER_EXTS.contains(&ext.as_str()) {
            let info = headers::probe(p, deep);
            lock(shared)
                .cache
                .insert(p.clone(), (mtime, size, info.clone()));
            result.insert(p.clone(), (size, info));
        } else {
            need_sips.push((p.clone(), mtime, size));
        }
    }
    if !need_sips.is_empty() {
        let got = sips_infos(
            &tools,
            &need_sips
                .iter()
                .map(|(p, _, _)| p.clone())
                .collect::<Vec<_>>(),
        );
        let mut st = lock(shared);
        for (p, mtime, size) in need_sips {
            let info = got.get(&p).cloned();
            st.cache.insert(p.clone(), (mtime, size, info.clone()));
            result.insert(p, (size, info));
        }
    }
    result
}

// ------------------------------------------------------------------ decorations

/// Scan the images of `folder` shown in `pane` and send their decorations
/// (unless a newer scan started: `gen` changed).
pub fn scan_folder(
    h: &PluginHandle,
    shared: &SharedRef,
    pane: PaneId,
    dir: VfsPath,
    folder: PathBuf,
    gen: u64,
    current: &AtomicU64,
) {
    let t0 = Instant::now();
    let mut names: Vec<String> = match std::fs::read_dir(&folder) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file() || t.is_symlink()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| meta::is_image(n))
            .collect(),
        Err(e) => {
            log::info!("image tools: scan {}: {e}", folder.display());
            Vec::new()
        }
    };
    names.sort();
    names.truncate(SCAN_LIMIT);
    let mut entries: Vec<(String, Decoration)> = Vec::new();
    for chunk in names.chunks(200) {
        if current.load(Ordering::Relaxed) != gen {
            return;
        }
        let paths: Vec<PathBuf> = chunk.iter().map(|n| folder.join(n)).collect();
        let infos = infos_for(shared, &paths, false);
        for (name, p) in chunk.iter().zip(&paths) {
            if let Some((size, info)) = infos.get(p) {
                if let Some(d) = meta::decoration(name, *size, info.as_ref()) {
                    entries.push((name.clone(), d));
                }
            }
        }
    }
    if current.load(Ordering::Relaxed) != gen {
        return;
    }
    if !names.is_empty() {
        log::debug!(
            "image tools: decorated {} images in {} ({} ms)",
            entries.len(),
            folder.display(),
            t0.elapsed().as_millis()
        );
    }
    h.send(HostRequest::SetDecorations { pane, dir, entries });
}

// ------------------------------------------------------------------ panel

/// The selected images of a local pane (existing files only).
pub fn selected_images(state: Option<&PaneState>, limit: usize) -> Vec<PathBuf> {
    let Some(s) = state.filter(|s| s.local_path.is_some()) else {
        return Vec::new();
    };
    s.selection
        .iter()
        .take(limit)
        .map(|p| PathBuf::from(p.as_str()))
        .filter(|p| meta::is_image(&path_str(p)) && p.is_file())
        .collect()
}

/// A file the panel can draw: the image itself, or a PNG made by sips for
/// HEIC/TIFF/AVIF (cached in the data folder, newest 200 kept).
pub fn preview_path(tools: &Tools, data_dir: &Path, path: &Path, mtime: i128) -> Option<PathBuf> {
    let ext = meta::ext_of(&path_str(path));
    if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "jpe" | "gif" | "webp" | "bmp"
    ) {
        return Some(path.to_path_buf());
    }
    if !tools.has("sips") {
        return None;
    }
    let folder = data_dir.join("previews");
    let digest = Sha256::digest(format!("{}|{mtime}", path.display()).as_bytes());
    let key: String = digest.iter().take(10).map(|b| format!("{b:02x}")).collect();
    let out = folder.join(format!("{key}.png"));
    if out.exists() {
        return Some(out);
    }
    let _ = std::fs::create_dir_all(&folder);
    let argv: Vec<String> = vec![
        "sips".into(),
        "-s".into(),
        "format".into(),
        "png".into(),
        "-Z".into(),
        PREVIEW_EDGE.to_string(),
        path_str(path),
        "--out".into(),
        path_str(&out),
    ];
    let (rc, _, err) = run(tools, &argv, Duration::from_secs(30), None);
    if rc != 0 || !out.exists() {
        log::info!(
            "image tools: preview of {} failed: {}",
            path.display(),
            err.trim()
        );
        return None;
    }
    prune_previews(&folder);
    Some(out)
}

fn prune_previews(folder: &Path) {
    let Ok(rd) = std::fs::read_dir(folder) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() <= PREVIEW_KEEP {
        return;
    }
    files.sort();
    for (_, p) in &files[..files.len() - PREVIEW_KEEP] {
        let _ = std::fs::remove_file(p);
    }
}

/// Everything the panel shows about one image (runs `mdls` for
/// HEIC/TIFF/AVIF and `exiftool` when installed).
pub fn details(shared: &SharedRef, path: &Path) -> Option<Details> {
    let (mtime, size) = stat(path)?;
    let ext = meta::ext_of(&path_str(path));
    let (tools, data_dir) = {
        let st = lock(shared);
        (st.tools.clone(), st.data_dir.clone())
    };
    let mut info = infos_for(shared, &[path.to_path_buf()], true)
        .remove(path)
        .and_then(|(_, i)| i);
    let mut sources: Vec<Exif> = vec![info.as_ref().map(|i| i.exif.clone()).unwrap_or_default()];
    let mut md = HashMap::new();
    if meta::SIPS_EXTS.contains(&ext.as_str()) && tools.has("mdls") {
        let (_, text, _) = run(
            &tools,
            &["mdls".into(), path_str(path)],
            Duration::from_secs(10),
            None,
        );
        md = meta::parse_mdls(&text);
        if info.is_none() {
            info = meta::info_from_mdls(&md, &ext);
        }
        sources.insert(0, meta::exif_from_mdls(&md));
    }
    if tools.has("exiftool") {
        let argv: Vec<String> = ["exiftool", "-json", "-n", "-q"]
            .map(String::from)
            .into_iter()
            .chain([path_str(path)])
            .collect();
        let (rc, text, _) = run(&tools, &argv, Duration::from_secs(20), None);
        if rc == 0 {
            sources.insert(0, meta::parse_exiftool(&text));
        }
    }
    let exif = Exif::merge(&sources.iter().collect::<Vec<_>>());
    if let Some(i) = info
        .as_mut()
        .filter(|_| meta::SIPS_EXTS.contains(&ext.as_str()))
    {
        if i.profile.is_none() {
            if let Some(meta::Md::Text(p)) = md.get("kMDItemProfileName") {
                i.profile = Some(p.clone());
            }
        }
    }
    let preview = preview_path(&tools, &data_dir, path, mtime);
    Some(Details {
        path: path.to_path_buf(),
        size,
        mtime,
        info,
        exif,
        preview,
    })
}

/// Rebuild the panel model and the command list for the active pane.
pub fn build_panel(h: &PluginHandle, shared: &SharedRef, state: Option<&PaneState>) {
    let images_200 = selected_images(state, 200);
    let datauri = images_200.len() == 1 && {
        let p = &images_200[0];
        ops::data_uri_ok(&path_str(p), stat(p).map(|s| s.1)).is_ok()
    };
    {
        let mut st = lock(shared);
        if st.images != images_200 || st.datauri != datauri {
            let changed = st.images.is_empty() != images_200.is_empty() || st.datauri != datauri;
            st.images = images_200;
            st.datauri = datauri;
            if changed {
                h.send(HostRequest::ContributionsChanged);
            }
        }
    }
    let model = match state {
        None => PanelModel::Waiting,
        Some(s) if s.local_path.is_none() => PanelModel::Remote,
        Some(s) => {
            let images = selected_images(Some(s), SCAN_LIMIT);
            match images.len() {
                0 => PanelModel::NoImage,
                1 => match details(shared, &images[0]) {
                    Some(d) => PanelModel::Single(Box::new(d)),
                    None => PanelModel::CantRead(
                        images[0]
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    ),
                },
                _ => {
                    let infos = infos_for(shared, &images, false);
                    PanelModel::Multi(
                        images
                            .iter()
                            .filter_map(|p| infos.get(p).map(|(s, i)| (p.clone(), *s, i.clone())))
                            .collect(),
                    )
                }
            }
        }
    };
    let mut st = lock(shared);
    if st.panel != model {
        st.panel = model;
        drop(st);
        h.request_repaint();
    }
}

/// The engine thread: debounced panel rebuilds.
pub fn engine_loop(h: PluginHandle, shared: SharedRef, rx: Receiver<Msg>) {
    let tools = Tools::find();
    log::info!(
        "image tools: tools: {}",
        TOOL_NAMES
            .iter()
            .filter(|n| tools.has(n))
            .copied()
            .collect::<Vec<_>>()
            .join(", ")
    );
    {
        let mut st = lock(&shared);
        st.tools = tools;
        st.tools_ready = true;
        let path = st.data_dir.join("options.json");
        if let Ok(text) = std::fs::read_to_string(path) {
            st.options = ops::parse_options(&text);
        }
    }
    let mut pending: Option<Option<PaneState>> = None;
    loop {
        let msg = if pending.is_some() {
            rx.recv_timeout(Duration::from_millis(60))
        } else {
            rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match msg {
            Ok(Msg::Panel { state }) => pending = Some(state),
            Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(state) = pending.take() {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        build_panel(&h, &shared, state.as_ref())
                    }));
                    if r.is_err() {
                        log::warn!("image tools: panel build panicked");
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ sheet

/// Prepare the sheet for `op` over `files` (infos, output names) — runs on
/// a worker thread, then the sheet opens.
pub fn prepare_modal(shared: &SharedRef, op: &str, files: Vec<PathBuf>, opts: Options) -> Modal {
    let infos = infos_for(shared, &files, false);
    let (have_cwebp, have_exiftool) = {
        let st = lock(shared);
        (st.tools.has("cwebp"), st.tools.has("exiftool"))
    };
    let mut previews = HashMap::new();
    if let Some(first) = files.first() {
        let own = ops::split_ext(&first.file_name().unwrap_or_default().to_string_lossy())
            .1
            .to_string();
        let mut exts = vec![own];
        exts.extend([".jpg", ".png", ".heic", ".tiff", ".webp"].map(String::from));
        for mode in ["suffix", "subfolder"] {
            for ext in &exts {
                let o = Options {
                    output: mode.into(),
                    ..opts.clone()
                };
                previews.insert(
                    (mode.to_string(), ext.clone()),
                    ops::output_preview(first, &o, Some(ext)),
                );
            }
        }
    }
    let name = |p: &Path| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let retina = files
        .iter()
        .take(6)
        .map(|f| match ops::retina_paths(f, &Default::default()) {
            Ok((two, one, rename)) => Ok((name(&two), name(&one), rename)),
            Err(e) => Err((name(f), e)),
        })
        .collect();
    Modal {
        op: op.into(),
        files,
        opts,
        confirm: false,
        infos,
        previews,
        retina,
        have_cwebp,
        have_exiftool,
    }
}

pub fn save_options(shared: &SharedRef) {
    let (dir, data) = {
        let st = lock(shared);
        (
            st.data_dir.clone(),
            serde_json::to_string_pretty(&st.options).unwrap_or_default(),
        )
    };
    let _ = std::fs::create_dir_all(&dir);
    if let Err(e) = std::fs::write(dir.join("options.json"), data) {
        log::warn!("image tools: options.json: {e}");
    }
}

// ------------------------------------------------------------------ jobs

fn temp_for(out: &Path) -> PathBuf {
    let name = out
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let (stem, ext) = ops::split_ext(&name);
    out.with_file_name(format!(".{stem}.rl-{}{ext}", std::process::id()))
}

/// Move `path` into ~/.Trash (same volume only); true if it worked.
fn move_to_trash(path: &Path) -> bool {
    let Some(trash) = dirs_home().map(|h| h.join(".Trash")) else {
        return false;
    };
    let same_dev = |a: &Path, b: &Path| {
        Some(std::fs::metadata(a).ok()?.dev() == std::fs::metadata(b).ok()?.dev())
    };
    if trash.is_dir() && same_dev(&trash, path) == Some(true) {
        let target = ops::unique(
            &trash.join(path.file_name().unwrap_or_default()),
            &Default::default(),
        );
        if std::fs::rename(path, target).is_ok() {
            return true;
        }
    }
    log::info!("image tools: couldn't move {} to the Trash", path.display());
    false
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn size_of(p: &Path) -> u64 {
    std::fs::metadata(p).map_or(0, |m| m.len())
}

fn last_line(s: &str) -> Option<String> {
    s.trim().lines().last().map(str::to_string)
}

/// One step: (bytes before, bytes after) or an error message.
fn do_step(
    tools: &Tools,
    job: &Job,
    step: &Step,
    overwrite: bool,
    trashed: &mut usize,
) -> Result<(u64, u64), String> {
    let (src, out) = (&step.src, step.out.as_ref().ok_or_else(String::new)?);
    let before = size_of(src);
    let run_tool = |argv: &[String]| -> Result<(), String> {
        let (rc, _, err) = run_cancellable(
            tools,
            argv,
            Duration::from_secs(600),
            None,
            Some(&job.cancelled),
        );
        if job.cancelled.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if rc != 0 {
            return Err(last_line(&err).unwrap_or_else(|| format!("{} failed ({rc})", argv[0])));
        }
        Ok(())
    };
    if let Work::Retina { two, rename } = &step.work {
        let info = headers::probe(src, false).or_else(|| {
            let (_, text, _) = run(
                tools,
                &[
                    "sips".into(),
                    "-g".into(),
                    "pixelWidth".into(),
                    "-g".into(),
                    "pixelHeight".into(),
                    path_str(src),
                ],
                Duration::from_secs(30),
                None,
            );
            meta::info_from_sips(
                meta::parse_sips(&text).get(&path_str(src)),
                &meta::ext_of(&path_str(src)),
            )
        });
        let info = info.ok_or_else(|| tr!("err.read_size").to_string())?;
        if *rename {
            std::fs::rename(src, two).map_err(|e| e.to_string())?;
        }
        if let Err(e) = run_tool(&ops::half_cmd(two, out, info.width, info.height)) {
            if *rename && !src.exists() {
                let _ = std::fs::rename(two, src);
            }
            return Err(e);
        }
        return Ok((before, before + size_of(out)));
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Outputs are unique names, except when overwriting the original: then
    // write a temporary file next to it and replace the original at the end
    // (exiftool does that by itself).
    let in_place = out == src || out.symlink_metadata().is_ok();
    let exiftool = matches!(&step.work, Work::Cmd(c) if c.first().is_some_and(|c| c == "exiftool"));
    let target = if in_place && !exiftool {
        temp_for(out)
    } else {
        out.clone()
    };
    let result = (|| -> Result<(), String> {
        match &step.work {
            Work::Strip => {
                let data = std::fs::read(src).map_err(|e| e.to_string())?;
                let fmt = headers::sniff(&data[..data.len().min(64)]).unwrap_or("?");
                let clean = ops::strip_bytes(&data, fmt)?;
                std::fs::write(&target, clean).map_err(|e| e.to_string())?;
            }
            Work::Cmd(argv) => {
                let argv = if target != *out {
                    ops::retarget(argv, out, &target).ok_or_else(String::new)?
                } else {
                    argv.clone()
                };
                run_tool(&argv)?;
                if !target.exists() {
                    return Err(format!("{} failed", argv[0]));
                }
            }
            _ => {}
        }
        if target != *out {
            std::fs::rename(&target, out).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        if target != *out {
            let _ = std::fs::remove_file(&target);
        }
        return Err(e);
    }
    if overwrite && out != src && out.exists() && move_to_trash(src) {
        *trashed += 1;
    }
    Ok((before, size_of(out)))
}

/// The verb of an operation in progress ("Resizing").
pub fn ing(op: &str) -> &'static str {
    match op {
        "resize" => tr!("ing.resize"),
        "convert" => tr!("ing.convert"),
        "rotate" => tr!("ing.rotate"),
        "strip" => tr!("ing.strip"),
        "optimize" => tr!("ing.optimize"),
        _ => tr!("ing.retina"),
    }
}

pub fn op_title(op: &str) -> &'static str {
    match op {
        "resize" => tr!("op.resize"),
        "convert" => tr!("op.convert"),
        "rotate" => tr!("op.rotate"),
        "strip" => tr!("op.strip"),
        "optimize" => tr!("op.optimize"),
        _ => tr!("op.retina"),
    }
}

fn done_text(op: &str, what: &str) -> String {
    match op {
        "resize" => tr!("done.resize", what = what),
        "convert" => tr!("done.convert", what = what),
        "rotate" => tr!("done.rotate", what = what),
        "strip" => tr!("done.strip", what = what),
        "optimize" => tr!("done.optimize", what = what),
        _ => tr!("done.retina", what = what),
    }
}

/// Plan and run a batch on this (worker) thread, then report.
pub fn run_job(h: &PluginHandle, shared: &SharedRef, m: Modal, lang: manta_plugin::i18n::Lang) {
    manta_plugin::i18n::set_thread_lang(Some(lang));
    let (tools, srgb) = (
        lock(shared).tools.clone(),
        Path::new(ops::SRGB_PROFILE).exists(),
    );
    let infos: HashMap<PathBuf, Option<Info>> = infos_for(shared, &m.files, false)
        .into_iter()
        .map(|(p, (_, i))| (p, i))
        .collect();
    let steps = match ops::plan(
        &m.op,
        &m.files,
        &m.opts,
        &infos,
        tools.has("cwebp"),
        tools.has("exiftool"),
        srgb,
    ) {
        Ok(s) => s,
        Err(e) => {
            h.send(HostRequest::Toast {
                level: ToastLevel::Warning,
                text: e,
            });
            return;
        }
    };
    let job = Arc::new(Job {
        op: m.op.clone(),
        total: steps.len(),
        index: AtomicUsize::new(0),
        current: Mutex::new(None),
        done: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
    });
    lock(shared).job = Some(job.clone());
    {
        let (h, job) = (h.clone(), job.clone());
        std::thread::spawn(move || {
            manta_plugin::i18n::set_thread_lang(Some(lang));
            std::thread::sleep(PROGRESS_AFTER);
            if !job.done.load(Ordering::Relaxed) && job.total > 1 {
                h.send(HostRequest::ActionToast {
                    level: ToastLevel::Info,
                    text: tr!(
                        "job.progress",
                        verb = ing(&job.op),
                        images = tr!("images", count = job.total)
                    ),
                    button: tr!("btn.cancel").into(),
                    command: "cancel".into(),
                });
            }
        });
    }
    let overwrite = m.opts.output == "overwrite";
    let (mut before, mut after, mut ok, mut trashed) = (0u64, 0u64, 0usize, 0usize);
    let mut skipped: Vec<String> = Vec::new();
    let mut errors: Vec<(PathBuf, String)> = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        if job.cancelled.load(Ordering::Relaxed) {
            break;
        }
        job.index.store(i, Ordering::Relaxed);
        *job.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(step.src.clone());
        h.request_repaint();
        if step.work == Work::Skip {
            skipped.push(step.note.clone().unwrap_or_default());
            continue;
        }
        match do_step(&tools, &job, step, overwrite, &mut trashed) {
            Ok((b, a)) => {
                before += b;
                after += a;
                ok += 1;
            }
            Err(e) => {
                if job.cancelled.load(Ordering::Relaxed) {
                    break;
                }
                log::info!("image tools: {} {}: {e}", m.op, step.src.display());
                errors.push((step.src.clone(), e));
            }
        }
    }
    job.done.store(true, Ordering::Relaxed);
    let what = if m.op == "retina" {
        tr!("sets", count = ok)
    } else {
        tr!("images", count = ok)
    };
    let name = |p: &Path| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let (level, text) = if job.cancelled.load(Ordering::Relaxed) {
        (ToastLevel::Info, tr!("job.cancelled", what = what))
    } else if !errors.is_empty() && ok == 0 {
        let (src, err) = &errors[0];
        (
            ToastLevel::Error,
            tr!(
                "job.failed",
                op = op_title(&m.op),
                name = name(src),
                error = err
            ),
        )
    } else {
        let mut text = done_text(&m.op, &what);
        if m.op != "retina" && ok > 0 {
            text += &tr!(
                "job.sizes",
                before = meta::human_size(before),
                after = meta::human_size(after)
            );
        }
        let mut level = ToastLevel::Success;
        if let Some((src, err)) = errors.first() {
            text += &format!(
                "\n{}",
                tr!(
                    "job.some_failed",
                    images = tr!("images", count = errors.len()),
                    name = name(src),
                    error = err
                )
            );
            level = ToastLevel::Warning;
        }
        if let Some(why) = skipped.first() {
            text += &format!(
                "\n{}",
                tr!(
                    "job.skipped",
                    images = tr!("images", count = skipped.len()),
                    why = why
                )
            );
        }
        if trashed > 0 {
            text += &format!(
                "\n{}",
                tr!("job.trashed", originals = tr!("originals", count = trashed))
            );
        }
        (level, text)
    };
    h.send(HostRequest::Toast { level, text });
    h.send(HostRequest::RefreshPane { pane: None });
}

/// Copy one image as a data URI (`pbcopy`), on this worker thread.
pub fn copy_data_uri(h: &PluginHandle, tools: &Tools, path: &Path) {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let toast = |level, text: String| h.send(HostRequest::Toast { level, text });
    if let Err(why) = ops::data_uri_ok(&name, stat(path).map(|s| s.1)) {
        toast(ToastLevel::Warning, tr!("toast.cant_copy", why = why));
        return;
    }
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            return toast(
                ToastLevel::Error,
                tr!("toast.cant_read", name = name, error = e),
            )
        }
    };
    let fmt = headers::sniff(&data[..data.len().min(64)])
        .or_else(|| meta::format_name(&meta::ext_of(&name)))
        .unwrap_or("?");
    let Some(uri) = ops::data_uri(&data, fmt) else {
        return toast(
            ToastLevel::Error,
            tr!("toast.cant_read", name = name, error = tr!("uri.format")),
        );
    };
    let (rc, _, err) = run(
        tools,
        &["pbcopy".into()],
        Duration::from_secs(10),
        Some(uri.as_bytes()),
    );
    if rc != 0 {
        return toast(ToastLevel::Error, tr!("toast.pbcopy", error = err.trim()));
    }
    let text = tr!(
        "toast.copied",
        name = name,
        size = meta::human_size(uri.len() as u64)
    );
    if data.len() as u64 > ops::DATA_URI_WARN {
        toast(
            ToastLevel::Warning,
            format!("{text}\n{}", tr!("toast.copied_big")),
        );
    } else {
        toast(ToastLevel::Success, text);
    }
}

/// The engine thread's mailbox.
pub type EngineTx = Sender<Msg>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headers::fixtures as B;

    fn shared(dir: &Path, tools: Tools) -> SharedRef {
        let s = Arc::new(Mutex::new(Shared::new(dir.to_path_buf())));
        lock(&s).tools = tools;
        s
    }

    #[test]
    fn cache_follows_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.png");
        std::fs::write(&p, B::png(30, 20, false, &[], 0)).unwrap();
        let s = shared(tmp.path(), Tools::default());
        let got = infos_for(&s, std::slice::from_ref(&p), false);
        assert_eq!(got[&p].1.as_ref().unwrap().width, 30);
        std::fs::write(&p, B::png(50, 20, false, &[], 0)).unwrap();
        let t = std::time::SystemTime::now() + Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(t)
            .unwrap();
        assert_eq!(
            infos_for(&s, std::slice::from_ref(&p), false)[&p]
                .1
                .as_ref()
                .unwrap()
                .width,
            50,
            "a new mtime re-reads"
        );
        assert!(infos_for(&s, &[tmp.path().join("gone.png")], false).is_empty());
    }

    #[test]
    fn heic_without_sips_has_no_info() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.heic");
        std::fs::write(&p, b"not really").unwrap();
        let s = shared(tmp.path(), Tools::default());
        assert_eq!(
            infos_for(&s, std::slice::from_ref(&p), false)[&p],
            (10, None)
        );
    }

    #[test]
    fn temp_names_hide_next_to_the_output() {
        let t = temp_for(Path::new("/a/photo.jpg"));
        assert_eq!(t.parent(), Some(Path::new("/a")));
        let n = t.file_name().unwrap().to_string_lossy().into_owned();
        assert!(n.starts_with(".photo.rl-") && n.ends_with(".jpg"), "{n}");
    }

    #[test]
    fn run_passes_argv_untouched() {
        let tools = Tools::default();
        let argv: Vec<String> = ["/bin/echo", "$(touch PWNED)", "a b"]
            .map(String::from)
            .to_vec();
        let (rc, out, _) = run(&tools, &argv, Duration::from_secs(5), None);
        assert_eq!((rc, out.as_str()), (0, "$(touch PWNED) a b\n"));
        let (rc, out, _) = run(
            &tools,
            &["/bin/cat".into()],
            Duration::from_secs(5),
            Some(b"data"),
        );
        assert_eq!((rc, out.as_str()), (0, "data"));
        let cancel = AtomicBool::new(true);
        let (rc, _, _) = run_cancellable(
            &tools,
            &["/bin/sleep".into(), "5".into()],
            Duration::from_secs(10),
            None,
            Some(&cancel),
        );
        assert_eq!(rc, -9);
    }

    #[test]
    fn strip_job_step_replaces_in_place_and_keeps_pixels() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("gps.png");
        let png = B::png(
            16,
            8,
            false,
            &[
                (b"eXIf", B::exif_photo(Some(B::PARIS), 1)),
                B::iccp_chunk("Display P3"),
            ],
            0,
        );
        std::fs::write(&src, &png).unwrap();
        let job = Job {
            op: "strip".into(),
            total: 1,
            index: AtomicUsize::new(0),
            current: Mutex::new(None),
            done: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        };
        let step = Step {
            src: src.clone(),
            out: Some(src.clone()),
            work: Work::Strip,
            note: None,
        };
        let mut trashed = 0;
        let (b, a) = do_step(&Tools::default(), &job, &step, true, &mut trashed).unwrap();
        assert!(a < b);
        let i = headers::probe(&src, false).unwrap();
        assert!(i.exif.gps.is_none());
        assert_eq!(i.profile.as_deref(), Some("Display P3"));
        assert_eq!(
            std::fs::read_dir(tmp.path()).unwrap().count(),
            1,
            "no temp file left"
        );
    }
}
