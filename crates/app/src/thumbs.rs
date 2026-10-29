//! Thumbnails for the Icons, Columns and Gallery views.
//!
//! - Made in the background by a small pool of worker threads (started on
//!   the first request); nothing is decoded on the UI thread.
//! - Requests are cancelled when nobody asks for them any more: a view asks
//!   for the thumbnails of the items it draws every frame, so items that
//!   scrolled out of view or belong to a folder that was left drop out of
//!   the queue after a couple of frames ([`Queue::pop`]).
//! - Images (PNG, JPEG, GIF, BMP, WebP) are decoded with the `image` crate,
//!   with their EXIF orientation applied. PDFs, videos, HEIC, documents …
//!   use macOS Quick Look (`QLThumbnailGenerator`); files without a
//!   thumbnail keep their type icon.
//! - Server files: icons only, unless "Load thumbnails for server files" is
//!   on; then images below [`REMOTE_MAX`] are downloaded in the background.
//! - Two caches: the egui textures in memory (least recently used ones are
//!   freed above [`TEXTURE_BUDGET`] bytes or after [`TEXTURE_IDLE`] unused),
//!   and PNGs on disk in `~/Library/Caches/Manta/thumbs`, keyed by path,
//!   modification time, size and pixel size ([`ThumbKey::disk_name`]).

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use manta_core::{Entry, SharedVfs, VfsPath};

use crate::icons::{self, FileKind};

/// Pixel sizes thumbnails are made at (the smallest one that covers the
/// requested size is used, so a slider drag doesn't make new ones).
pub const BUCKETS: [u32; 5] = [64, 128, 256, 512, 1024];
/// Server images above this size are never downloaded for a thumbnail.
pub const REMOTE_MAX: u64 = 5 * 1024 * 1024;
/// Memory for thumbnail textures (RGBA bytes).
pub const TEXTURE_BUDGET: usize = 160 * 1024 * 1024;
/// A texture nobody drew for this long is freed.
pub const TEXTURE_IDLE: Duration = Duration::from_secs(30);
/// A request nobody asked for in this many frames is dropped.
pub const KEEP_FRAMES: u64 = 2;
/// Textures uploaded per frame at most (keeps scrolling smooth).
const UPLOADS_PER_FRAME: usize = 12;
/// Remembered failures (files without a thumbnail).
const MAX_FAILED: usize = 20_000;

/// The smallest bucket ≥ `px` (the largest one above it).
pub fn bucket_for(px: f32) -> u32 {
    BUCKETS
        .iter()
        .copied()
        .find(|b| *b as f32 >= px)
        .unwrap_or(BUCKETS[BUCKETS.len() - 1])
}

/// What identifies a thumbnail: where the file is, its path, modification
/// time and size (a changed file gets a new thumbnail), and the pixel size.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThumbKey {
    /// "" for this Mac, else the backend's label ("deploy@web (SFTP)").
    pub origin: String,
    pub path: String,
    /// Seconds since the epoch (0 = unknown).
    pub mtime: i64,
    pub size: u64,
    pub px: u32,
}

impl ThumbKey {
    pub fn for_entry(origin: &str, e: &Entry, px: u32) -> Self {
        ThumbKey {
            origin: origin.to_string(),
            path: e.path.to_string(),
            mtime: e.modified.map(|t| t.timestamp()).unwrap_or(0),
            size: e.size,
            px,
        }
    }

    /// The disk cache's file name: a stable hash (FNV-1a, unlike std's
    /// hasher it never changes between Rust versions) plus the size.
    pub fn disk_name(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |bytes: &[u8]| {
            for b in bytes {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            h ^= 0xff;
            h = h.wrapping_mul(0x0100_0000_01b3);
        };
        feed(self.origin.as_bytes());
        feed(self.path.as_bytes());
        feed(&self.mtime.to_le_bytes());
        feed(&self.size.to_le_bytes());
        format!("{h:016x}-{}.png", self.px)
    }
}

/// How a thumbnail is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Decoded with the `image` crate.
    Image,
    /// macOS Quick Look (PDFs, videos, HEIC, documents …).
    QuickLook,
    /// An app bundle's icon (Quick Look, like Finder shows it).
    AppIcon,
    /// The icon macOS shows for a file type (by extension, for any file,
    /// local or not; Quick Look of an empty stand-in file).
    TypeIcon,
}

/// The cache key of the type icon for files named `*.ext` ("" for none).
pub fn type_key(ext: &str, px: u32) -> ThumbKey {
    ThumbKey {
        origin: "type-icon".into(),
        path: ext.to_string(),
        mtime: 0,
        size: 0,
        px,
    }
}

/// Formats the `image` crate decodes here.
pub fn decodable_image(name: &str) -> bool {
    matches!(
        icons::extension(name).as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp"
    )
}

/// A macOS app (`Safari.app`): a folder shown with its own icon.
pub fn is_app_bundle(e: &Entry) -> bool {
    e.is_dir() && e.name.len() > 4 && icons::extension(&e.name) == "app"
}

/// The app whose icon an app's folder in iCloud Drive shows, like in
/// Finder (`…/iCloud~md~obsidian/Documents` -> `Obsidian.app`). `None`
/// for other folders or when the app isn't installed (plain folder).
/// Remembered: views ask every frame.
pub fn icloud_app(folder: &Path) -> Option<PathBuf> {
    let (docs, container) = manta_vfs::icloud::app_library_of(folder)?;
    if docs != folder {
        return None;
    }
    let id = manta_vfs::icloud::bundle_id(&container)?;
    static APPS: Mutex<Option<HashMap<String, Option<PathBuf>>>> = Mutex::new(None);
    let mut apps = APPS.lock().unwrap_or_else(|e| e.into_inner());
    apps.get_or_insert_with(HashMap::new)
        .entry(id)
        .or_insert_with_key(|id| app_for_bundle_id(id))
        .clone()
}

#[cfg(target_os = "macos")]
fn app_for_bundle_id(id: &str) -> Option<PathBuf> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;
    objc2::rc::autoreleasepool(|_| {
        let url = NSWorkspace::sharedWorkspace()
            .URLForApplicationWithBundleIdentifier(&NSString::from_str(id))?;
        Some(PathBuf::from(url.path()?.to_string()))
    })
}

#[cfg(not(target_os = "macos"))]
fn app_for_bundle_id(_id: &str) -> Option<PathBuf> {
    None
}

/// How an icon file is drawn: an app bundle's icon, or the image of an
/// `.icns` file (a volume's custom icon).
pub fn icon_file_job(file: PathBuf) -> (Source, Method) {
    let method = if file.extension().is_some_and(|x| x == "app") {
        Method::AppIcon
    } else {
        Method::QuickLook
    };
    (Source::Local(file), method)
}

/// How `e` gets a thumbnail, if it can have one (local files).
pub fn method_for(e: &Entry) -> Option<Method> {
    if is_app_bundle(e) {
        return cfg!(target_os = "macos").then_some(Method::AppIcon);
    }
    if e.is_dir() || e.size == 0 {
        return None;
    }
    if decodable_image(&e.name) {
        return Some(Method::Image);
    }
    let ql = matches!(
        icons::file_kind(&e.name, false),
        FileKind::Image
            | FileKind::Video
            | FileKind::Pdf
            | FileKind::Document
            | FileKind::Spreadsheet
            | FileKind::Presentation
    );
    (ql && cfg!(target_os = "macos")).then_some(Method::QuickLook)
}

/// Where the file's bytes come from.
#[derive(Clone)]
pub enum Source {
    Local(PathBuf),
    /// A file type (extension) for [`Method::TypeIcon`].
    Type(String),
    /// A server file, downloaded (only images below [`REMOTE_MAX`]).
    Remote(SharedVfs, VfsPath),
}

/// A thumbnail as a view sees it.
#[derive(Clone)]
pub enum Thumb {
    Ready(egui::TextureHandle),
    /// Being made (draw the icon meanwhile).
    Pending,
    /// No thumbnail for this file: draw its icon.
    Unavailable,
}

struct Job {
    key: ThumbKey,
    source: Source,
    method: Method,
}

/// The request queue: newest requests first (what the user looks at now),
/// requests nobody asked for in [`KEEP_FRAMES`] frames are dropped.
#[derive(Default)]
pub struct Queue {
    jobs: Vec<Job>,
    queued: HashSet<ThumbKey>,
    in_flight: HashSet<ThumbKey>,
    /// The last frame each key was asked for.
    wanted: HashMap<ThumbKey, u64>,
    pub frame: u64,
    /// Requests dropped because nobody wanted them any more.
    pub cancelled: u64,
    closed: bool,
    /// Test knob: workers take nothing while set.
    #[cfg(test)]
    paused: bool,
}

impl Queue {
    /// Ask for `key` this frame; queues a job unless one is queued or
    /// running. Returns whether a new job was queued.
    fn want(&mut self, key: &ThumbKey, make: impl FnOnce() -> Option<(Source, Method)>) -> bool {
        self.wanted.insert(key.clone(), self.frame);
        if self.queued.contains(key) || self.in_flight.contains(key) {
            return false;
        }
        let Some((source, method)) = make() else {
            return false;
        };
        self.queued.insert(key.clone());
        self.jobs.push(Job {
            key: key.clone(),
            source,
            method,
        });
        true
    }

    fn is_wanted(&self, key: &ThumbKey) -> bool {
        self.wanted
            .get(key)
            .is_some_and(|f| f + KEEP_FRAMES >= self.frame)
    }

    /// The next job someone still wants (newest first); stale ones are
    /// dropped on the way.
    fn pop(&mut self) -> Option<Job> {
        #[cfg(test)]
        if self.paused {
            return None;
        }
        while let Some(job) = self.jobs.pop() {
            self.queued.remove(&job.key);
            if self.is_wanted(&job.key) {
                self.in_flight.insert(job.key.clone());
                return Some(job);
            }
            self.cancelled += 1;
        }
        None
    }

    fn done(&mut self, key: &ThumbKey) {
        self.in_flight.remove(key);
    }

    /// Next frame: forget what nobody asked for recently and drop its jobs.
    fn next_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        self.wanted.retain(|_, f| *f + KEEP_FRAMES >= frame);
        let before = self.jobs.len();
        let wanted = &self.wanted;
        let queued = &mut self.queued;
        self.jobs.retain(|j| {
            let keep = wanted.contains_key(&j.key);
            if !keep {
                queued.remove(&j.key);
            }
            keep
        });
        self.cancelled += (before - self.jobs.len()) as u64;
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.jobs.len()
    }
}

struct Shared {
    queue: Mutex<Queue>,
    cv: Condvar,
    disk: Option<PathBuf>,
    ctx: egui::Context,
}

struct Done {
    key: ThumbKey,
    image: Result<egui::ColorImage, String>,
}

struct Tex {
    handle: egui::TextureHandle,
    bytes: usize,
    last_frame: u64,
    last_used: Instant,
}

#[derive(Default)]
struct State {
    tex: HashMap<ThumbKey, Tex>,
    bytes: usize,
    failed: HashSet<ThumbKey>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

/// The thumbnail service (one per app, shared by all panes).
pub struct Thumbs {
    shared: Arc<Shared>,
    state: Mutex<State>,
    tx: mpsc::Sender<Done>,
    rx: Mutex<mpsc::Receiver<Done>>,
    workers: usize,
    /// Thumbnails made (not from the disk cache), for tests and logs.
    pub made: Arc<AtomicU64>,
}

impl Thumbs {
    /// `disk`: the disk cache folder (`None`: memory only).
    pub fn new(ctx: &egui::Context, disk: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel();
        if let Some(d) = &disk {
            let d = d.clone();
            let _ = std::thread::Builder::new()
                .name("manta-thumbs-prune".into())
                .spawn(move || prune_disk(&d, 512 << 20, 384 << 20));
        }
        let workers = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(2, 4));
        Thumbs {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                cv: Condvar::new(),
                disk,
                ctx: ctx.clone(),
            }),
            state: Mutex::new(State::default()),
            tx,
            rx: Mutex::new(rx),
            workers,
            made: Arc::new(AtomicU64::new(0)),
        }
    }

    /// `~/Library/Caches/Manta/thumbs`.
    pub fn default_disk_dir() -> Option<PathBuf> {
        dirs::cache_dir().map(|d| d.join("Manta").join("thumbs"))
    }

    fn ensure_workers(&self, st: &mut State) {
        if !st.workers.is_empty() {
            return;
        }
        for i in 0..self.workers {
            let shared = self.shared.clone();
            let tx = self.tx.clone();
            let made = self.made.clone();
            if let Ok(h) = std::thread::Builder::new()
                .name(format!("manta-thumbs-{i}"))
                .spawn(move || worker(shared, tx, made))
            {
                st.workers.push(h);
            }
        }
    }

    /// The thumbnail for `key`. Not ready yet: queues it (`make` says how,
    /// `None` = it has none) and returns [`Thumb::Pending`].
    pub fn get(&self, key: &ThumbKey, make: impl FnOnce() -> Option<(Source, Method)>) -> Thumb {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let frame = self.frame();
        if let Some(t) = st.tex.get_mut(key) {
            t.last_frame = frame;
            t.last_used = Instant::now();
            return Thumb::Ready(t.handle.clone());
        }
        if st.failed.contains(key) {
            return Thumb::Unavailable;
        }
        let mut none = false;
        let queued = {
            let mut q = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            q.want(key, || {
                let m = make();
                none = m.is_none();
                m
            })
        };
        if none {
            return Thumb::Unavailable;
        }
        if queued {
            self.ensure_workers(&mut st);
            self.shared.cv.notify_one();
        }
        Thumb::Pending
    }

    /// A ready texture of `key` or of a bigger / smaller bucket of the same
    /// file (shown while the right size is made, e.g. while zooming).
    pub fn get_any(
        &self,
        key: &ThumbKey,
        make: impl FnOnce() -> Option<(Source, Method)>,
    ) -> Thumb {
        let t = self.get(key, make);
        if !matches!(t, Thumb::Pending) {
            return t;
        }
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        for px in BUCKETS.iter().rev() {
            let k = ThumbKey {
                px: *px,
                ..key.clone()
            };
            if let Some(t) = st.tex.get(&k) {
                return Thumb::Ready(t.handle.clone());
            }
        }
        Thumb::Pending
    }

    fn frame(&self) -> u64 {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frame
    }

    /// Once per frame, before the views draw: take finished thumbnails
    /// (uploading a few per frame), drop requests nobody wants any more and
    /// free textures above the memory budget.
    pub fn begin_frame(&self, ctx: &egui::Context) {
        let frame = {
            let mut q = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            q.next_frame();
            q.frame
        };
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let rx = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        let mut uploaded = 0;
        while uploaded < UPLOADS_PER_FRAME {
            let Ok(done) = rx.try_recv() else { break };
            match done.image {
                Ok(img) => {
                    let bytes = img.pixels.len() * 4;
                    let name = format!("thumb:{}@{}", done.key.path, done.key.px);
                    let handle = ctx.load_texture(name, img, egui::TextureOptions::LINEAR);
                    st.bytes += bytes;
                    if let Some(old) = st.tex.insert(
                        done.key,
                        Tex {
                            handle,
                            bytes,
                            last_frame: frame,
                            last_used: Instant::now(),
                        },
                    ) {
                        st.bytes -= old.bytes;
                    }
                    uploaded += 1;
                }
                Err(e) => {
                    log::debug!("thumbs: none for {}: {e}", done.key.path);
                    if st.failed.len() > MAX_FAILED {
                        st.failed.clear();
                    }
                    st.failed.insert(done.key);
                }
            }
        }
        if uploaded == UPLOADS_PER_FRAME {
            ctx.request_repaint();
        }
        evict(&mut st, frame, TEXTURE_BUDGET, TEXTURE_IDLE);
    }

    /// Bytes of thumbnail textures held now.
    #[cfg(test)]
    pub fn texture_bytes(&self) -> usize {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).bytes
    }

    /// Textures held now.
    #[cfg(test)]
    pub fn texture_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tex
            .len()
    }

    /// Requests waiting and requests dropped so far (tests).
    #[cfg(test)]
    pub fn queue_stats(&self) -> (usize, u64) {
        let q = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        (q.len(), q.cancelled)
    }

    /// Test knob: stop the workers from taking jobs (what is queued stays
    /// queued until cancelled or resumed), so tests don't race them.
    #[cfg(test)]
    pub fn set_paused(&self, paused: bool) {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .paused = paused;
        self.shared.cv.notify_all();
    }

    /// Wait until nothing is queued or running (tests, renders).
    #[cfg(test)]
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            {
                let q = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
                if q.jobs.is_empty() && q.in_flight.is_empty() {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

impl Drop for Thumbs {
    fn drop(&mut self) {
        if let Ok(mut q) = self.shared.queue.lock() {
            q.closed = true;
            q.jobs.clear();
        }
        self.shared.cv.notify_all();
    }
}

/// Free textures not drawn this frame: idle ones, then the least recently
/// used until the total is within `budget`.
fn evict(st: &mut State, frame: u64, budget: usize, idle: Duration) {
    let idle_keys: Vec<ThumbKey> = st
        .tex
        .iter()
        .filter(|(_, t)| t.last_frame < frame && t.last_used.elapsed() > idle)
        .map(|(k, _)| k.clone())
        .collect();
    for k in idle_keys {
        if let Some(t) = st.tex.remove(&k) {
            st.bytes -= t.bytes;
        }
    }
    if st.bytes <= budget {
        return;
    }
    let mut old: Vec<(u64, ThumbKey)> = st
        .tex
        .iter()
        .filter(|(_, t)| t.last_frame + 1 < frame)
        .map(|(k, t)| (t.last_frame, k.clone()))
        .collect();
    old.sort_by_key(|(f, _)| *f);
    for (_, k) in old {
        if st.bytes <= budget {
            break;
        }
        if let Some(t) = st.tex.remove(&k) {
            st.bytes -= t.bytes;
        }
    }
}

fn worker(shared: Arc<Shared>, tx: mpsc::Sender<Done>, made: Arc<AtomicU64>) {
    loop {
        let job = {
            let mut q = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if q.closed {
                    return;
                }
                if let Some(j) = q.pop() {
                    break j;
                }
                q = shared.cv.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let image = make_thumb(&job, shared.disk.as_deref(), &made);
        if let Ok(mut q) = shared.queue.lock() {
            q.done(&job.key);
        }
        if tx
            .send(Done {
                key: job.key,
                image,
            })
            .is_err()
        {
            return;
        }
        shared.ctx.request_repaint();
    }
}

fn make_thumb(
    job: &Job,
    disk: Option<&Path>,
    made: &AtomicU64,
) -> Result<egui::ColorImage, String> {
    let cached = disk.map(|d| d.join(job.key.disk_name()));
    if let Some(img) = cached.as_ref().and_then(|p| image::open(p).ok()) {
        return Ok(color_image(img.to_rgba8()));
    }
    let px = job.key.px;
    let rgba = match (&job.source, job.method) {
        (Source::Local(path), Method::Image) => {
            let r = image::ImageReader::open(path).map_err(|e| e.to_string())?;
            decode(r, px)?
        }
        (Source::Remote(fs, path), Method::Image) => {
            if job.key.size > REMOTE_MAX {
                return Err("too large to download".into());
            }
            let mut bytes = Vec::new();
            fs.open_read(path)
                .map_err(|e| e.to_string())?
                .take(REMOTE_MAX + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            decode(image::ImageReader::new(std::io::Cursor::new(bytes)), px)?
        }
        (Source::Local(path), Method::QuickLook) => quicklook(path, px, false)?,
        (Source::Local(path), Method::AppIcon) => quicklook(path, px, true)?,
        (Source::Type(ext), _) => quicklook(&type_stand_in(ext)?, px, true)?,
        (_, Method::TypeIcon) => return Err("a type icon needs a type".into()),
        (Source::Remote(..), Method::QuickLook | Method::AppIcon) => {
            return Err("Quick Look needs a local file".into())
        }
    };
    made.fetch_add(1, Ordering::Relaxed);
    if let Some(p) = cached {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = p.with_extension(format!("tmp{}", std::process::id()));
        if rgba.save_with_format(&tmp, image::ImageFormat::Png).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    Ok(color_image(rgba))
}

fn color_image(rgba: image::RgbaImage) -> egui::ColorImage {
    let size = [rgba.width() as usize, rgba.height() as usize];
    egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
}

/// Decode with the EXIF orientation applied, scaled to fit `px` × `px`.
fn decode<R: std::io::BufRead + std::io::Seek>(
    r: image::ImageReader<R>,
    px: u32,
) -> Result<image::RgbaImage, String> {
    use image::ImageDecoder;
    let mut r = r.with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(1 << 30);
    r.limits(limits);
    let mut dec = r.into_decoder().map_err(|e| e.to_string())?;
    let orientation = dec
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(dec).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    if img.width() > px || img.height() > px {
        img = img.thumbnail(px, px);
    }
    Ok(img.to_rgba8())
}

/// Keep the disk cache below `max` bytes: oldest files go until `target`.
fn prune_disk(dir: &Path, max: u64, target: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= max {
        return;
    }
    files.sort_by_key(|f| f.0);
    for (_, len, path) in files {
        if total <= target {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

/// An empty file named `icon.<ext>` in the temp folder: Quick Look gives
/// its icon, the one macOS shows for that type.
fn type_stand_in(ext: &str) -> Result<PathBuf, String> {
    let ok = ext
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '+'));
    if !ok || ext.len() > 32 {
        return Err(format!("odd extension {ext:?}"));
    }
    let dir = std::env::temp_dir().join("manta-type-icons");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(if ext.is_empty() {
        "icon".to_string()
    } else {
        format!("icon.{ext}")
    });
    if !path.exists() {
        std::fs::write(&path, b"").map_err(|e| e.to_string())?;
    }
    Ok(path)
}

/// A Quick Look thumbnail of a local file (PDF, video, HEIC, document …).
#[cfg(target_os = "macos")]
fn quicklook(path: &Path, px: u32, icon: bool) -> Result<image::RgbaImage, String> {
    use block2::RcBlock;
    use objc2::AllocAnyThread;
    use objc2_core_foundation::CGSize;
    use objc2_foundation::{NSError, NSString, NSURL};
    use objc2_quick_look_thumbnailing::{
        QLThumbnailGenerationRequest, QLThumbnailGenerationRequestRepresentationTypes,
        QLThumbnailGenerator, QLThumbnailRepresentation,
    };

    let (tx, rx) = mpsc::channel::<Result<image::RgbaImage, String>>();
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    // SAFETY: plain calls into QuickLookThumbnailing; the request and the
    // block live until the generator calls back (we wait for it below).
    unsafe {
        let req = QLThumbnailGenerationRequest::initWithFileAtURL_size_scale_representationTypes(
            QLThumbnailGenerationRequest::alloc(),
            &url,
            CGSize {
                width: px as f64,
                height: px as f64,
            },
            1.0,
            if icon {
                QLThumbnailGenerationRequestRepresentationTypes::Icon
            } else {
                QLThumbnailGenerationRequestRepresentationTypes::Thumbnail
            },
        );
        let block = RcBlock::new(
            move |rep: *mut QLThumbnailRepresentation, _err: *mut NSError| {
                let out = match rep.as_ref() {
                    Some(rep) => cgimage_rgba(&rep.CGImage()),
                    None => Err("no thumbnail".to_string()),
                };
                let _ = tx.send(out);
            },
        );
        QLThumbnailGenerator::sharedGenerator()
            .generateBestRepresentationForRequest_completionHandler(&req, &block);
        rx.recv_timeout(Duration::from_secs(15))
            .map_err(|_| "Quick Look timed out".to_string())?
    }
}

#[cfg(not(target_os = "macos"))]
fn quicklook(_path: &Path, _px: u32, _icon: bool) -> Result<image::RgbaImage, String> {
    Err("Quick Look is macOS only".into())
}

/// Draw a CGImage into an RGBA buffer (un-premultiplied for `image`).
#[cfg(target_os = "macos")]
fn cgimage_rgba(cg: &objc2_core_graphics::CGImage) -> Result<image::RgbaImage, String> {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
        CGImageByteOrderInfo,
    };
    let w = CGImage::width(Some(cg));
    let h = CGImage::height(Some(cg));
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(format!("bad thumbnail size {w}×{h}"));
    }
    let mut buf = vec![0u8; w * h * 4];
    let space = CGColorSpace::new_device_rgb().ok_or("no colour space")?;
    // SAFETY: `buf` outlives the context, which is dropped before reading.
    unsafe {
        let ctx = CGBitmapContextCreate(
            buf.as_mut_ptr().cast(),
            w,
            h,
            8,
            w * 4,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
        )
        .ok_or("no bitmap context")?;
        CGContext::draw_image(
            Some(&ctx),
            CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize {
                    width: w as f64,
                    height: h as f64,
                },
            },
            Some(cg),
        );
    }
    for px in buf.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(w as u32, h as u32, buf).ok_or_else(|| "bad buffer".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_core::EntryKind;

    fn entry(name: &str, size: u64, mtime: i64) -> Entry {
        Entry {
            name: name.into(),
            path: VfsPath::new(&format!("/pics/{name}")),
            kind: EntryKind::File,
            size,
            modified: chrono::DateTime::from_timestamp(mtime, 0),
            permissions: None,
            link_target: None,
            hidden: false,
        }
    }

    #[test]
    fn buckets_cover_the_requested_size() {
        assert_eq!(bucket_for(40.0), 64);
        assert_eq!(bucket_for(96.0), 128);
        assert_eq!(bucket_for(256.0), 256);
        assert_eq!(bucket_for(257.0), 512);
        assert_eq!(bucket_for(5000.0), 1024);
    }

    #[test]
    fn cache_key_follows_path_mtime_and_size() {
        let a = ThumbKey::for_entry("", &entry("a.jpg", 100, 1_700_000_000), 256);
        assert_eq!(
            a.disk_name(),
            ThumbKey::for_entry("", &entry("a.jpg", 100, 1_700_000_000), 256).disk_name(),
            "stable"
        );
        assert!(a.disk_name().ends_with("-256.png"));
        let others = [
            ThumbKey::for_entry("", &entry("a.jpg", 100, 1_700_000_001), 256),
            ThumbKey::for_entry("", &entry("a.jpg", 101, 1_700_000_000), 256),
            ThumbKey::for_entry("", &entry("b.jpg", 100, 1_700_000_000), 256),
            ThumbKey::for_entry(
                "deploy@web (SFTP)",
                &entry("a.jpg", 100, 1_700_000_000),
                256,
            ),
            ThumbKey::for_entry("", &entry("a.jpg", 100, 1_700_000_000), 512),
        ];
        for o in &others {
            assert_ne!(o, &a);
            assert_ne!(o.disk_name(), a.disk_name(), "{o:?}");
        }
        // Known value: the name must not change between builds (old cache
        // files would be orphaned).
        let k = ThumbKey {
            origin: String::new(),
            path: "/x".into(),
            mtime: 1,
            size: 2,
            px: 64,
        };
        assert_eq!(
            k.disk_name(),
            ThumbKey {
                origin: String::new(),
                path: "/x".into(),
                mtime: 1,
                size: 2,
                px: 64
            }
            .disk_name()
        );
    }

    #[test]
    fn methods_by_type() {
        assert_eq!(method_for(&entry("a.JPG", 10, 0)), Some(Method::Image));
        assert_eq!(method_for(&entry("a.webp", 10, 0)), Some(Method::Image));
        assert_eq!(method_for(&entry("a.txt", 10, 0)), None);
        assert_eq!(
            method_for(&entry("empty.png", 0, 0)),
            None,
            "an empty file has nothing to show"
        );
        if cfg!(target_os = "macos") {
            assert_eq!(
                method_for(&entry("doc.pdf", 10, 0)),
                Some(Method::QuickLook)
            );
            assert_eq!(
                method_for(&entry("clip.mov", 10, 0)),
                Some(Method::QuickLook)
            );
            assert_eq!(
                method_for(&entry("IMG_1.HEIC", 10, 0)),
                Some(Method::QuickLook)
            );
        }
    }

    #[test]
    fn apps_show_their_own_icon() {
        let dir = |name: &str| Entry {
            kind: EntryKind::Directory,
            ..entry(name, 0, 0)
        };
        let want = cfg!(target_os = "macos").then_some(Method::AppIcon);
        assert_eq!(method_for(&dir("Safari.app")), want);
        assert_eq!(method_for(&dir("Old.APP")), want);
        assert_eq!(method_for(&dir("app")), None, "a folder named app");
        assert_eq!(method_for(&dir(".app")), None);
        assert_eq!(method_for(&entry("notes.app", 10, 0)), None, "a file");
    }

    /// Quick Look draws a real app's icon (as Finder shows it).
    #[cfg(target_os = "macos")]
    #[test]
    fn quick_look_makes_an_app_icon() {
        let app = Path::new("/System/Applications/Calculator.app");
        if !app.is_dir() {
            return;
        }
        let img = quicklook(app, 64, true).expect("app icon");
        assert!(
            img.width() >= 32 && img.height() >= 32,
            "{}×{}",
            img.width(),
            img.height()
        );
        assert!(img.pixels().any(|p| p[3] > 0), "not blank");
    }

    /// A volume's custom icon (`.icns`) comes out as its image.
    #[cfg(target_os = "macos")]
    #[test]
    fn quick_look_draws_an_icns_file() {
        let icns = Path::new(
            "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericFolderIcon.icns",
        );
        if !icns.is_file() {
            return;
        }
        let (source, method) = icon_file_job(icns.to_path_buf());
        assert_eq!(method, Method::QuickLook);
        let Source::Local(path) = source else {
            panic!("a local file")
        };
        let img = quicklook(&path, 64, false).expect("icns image");
        assert!(img.width() >= 32, "{}", img.width());
        assert!(img.pixels().any(|p| p[3] > 0), "blank");
    }

    /// Quick Look gives the icon macOS shows for a file type.
    #[cfg(target_os = "macos")]
    #[test]
    fn quick_look_makes_type_icons() {
        for ext in ["pdf", "txt", ""] {
            let img = quicklook(&type_stand_in(ext).unwrap(), 64, true).expect(ext);
            assert!(img.width() >= 32, "{ext}: {}", img.width());
            assert!(img.pixels().any(|p| p[3] > 0), "{ext}: blank");
        }
        assert!(type_stand_in("../x").is_err(), "no paths as extensions");
    }

    fn job_source() -> Option<(Source, Method)> {
        Some((Source::Local(PathBuf::from("/nope")), Method::Image))
    }

    #[test]
    fn queue_drops_what_scrolled_out_of_view() {
        let mut q = Queue::default();
        let k = |i: u64| ThumbKey {
            origin: String::new(),
            path: format!("/{i}"),
            mtime: 0,
            size: 1,
            px: 128,
        };
        q.next_frame();
        for i in 0..10 {
            assert!(q.want(&k(i), job_source));
        }
        assert!(!q.want(&k(3), job_source), "queued once");
        // Next frames: only 8 and 9 are still on screen.
        for _ in 0..3 {
            q.next_frame();
            q.want(&k(8), job_source);
            q.want(&k(9), job_source);
        }
        assert_eq!(q.len(), 2, "the other eight were cancelled");
        assert_eq!(q.cancelled, 8);
        // Newest first.
        let a = q.pop().unwrap();
        assert_eq!(a.key, k(9));
        assert!(!q.want(&k(9), job_source), "running: not queued again");
        q.done(&a.key);
        assert!(q.pop().is_some());
        assert!(q.pop().is_none());
    }

    #[test]
    fn queue_skips_jobs_wanted_too_long_ago() {
        let mut q = Queue::default();
        let k = ThumbKey {
            origin: String::new(),
            path: "/a".into(),
            mtime: 0,
            size: 1,
            px: 128,
        };
        q.want(&k, job_source);
        // The folder changed: frames pass without anyone asking.
        q.frame += 5;
        assert!(q.pop().is_none(), "a stale job is not made");
        assert_eq!(q.cancelled, 1);
    }

    fn write_png(path: &Path, w: u32, h: u32) {
        let img = image::RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 128, 255])
        });
        img.save(path).unwrap();
    }

    #[test]
    fn thumbnails_are_made_in_the_background_and_cached_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let pic = dir.path().join("wide.png");
        write_png(&pic, 400, 200);
        let e = Entry {
            path: VfsPath::new(&pic.to_string_lossy()),
            ..entry("wide.png", std::fs::metadata(&pic).unwrap().len(), 1)
        };
        let ctx = egui::Context::default();
        let key = ThumbKey::for_entry("", &e, 128);
        let src = || Some((Source::Local(pic.clone()), Method::Image));
        let thumbs = Thumbs::new(&ctx, Some(cache.clone()));
        let mut ready = None;
        for _ in 0..500 {
            thumbs.begin_frame(&ctx);
            if let Thumb::Ready(t) = thumbs.get(&key, src) {
                ready = Some(t);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let t = ready.expect("thumbnail made");
        assert_eq!(t.size(), [128, 64], "fits 128 px, aspect kept");
        assert!(
            cache.join(key.disk_name()).exists(),
            "written to the disk cache"
        );
        assert_eq!(thumbs.made.load(Ordering::Relaxed), 1);
        assert!(thumbs.texture_bytes() > 0);

        // A new service (next launch) reads it from disk.
        let again = Thumbs::new(&ctx, Some(cache));
        for _ in 0..500 {
            again.begin_frame(&ctx);
            if matches!(again.get(&key, src), Thumb::Ready(_)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(again.get(&key, src), Thumb::Ready(_)));
        assert_eq!(again.made.load(Ordering::Relaxed), 0, "from the disk cache");
    }

    #[test]
    fn exif_orientation_is_applied() {
        // A 2×1 JPEG tagged "rotate 90°" decodes as 1×2.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("o.jpg");
        let img = image::RgbImage::from_pixel(40, 20, image::Rgb([200, 10, 10]));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&img)
            .unwrap();
        // Insert an APP1 Exif segment with Orientation = 6 after SOI.
        let exif: Vec<u8> = {
            let mut tiff = vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1];
            tiff.extend([0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0]);
            tiff.extend([0, 0, 0, 0]);
            let mut seg = b"Exif\0\0".to_vec();
            seg.extend(tiff);
            let mut out = vec![0xFF, 0xE1];
            out.extend(((seg.len() + 2) as u16).to_be_bytes());
            out.extend(seg);
            out
        };
        let mut file = jpeg[..2].to_vec();
        file.extend(exif);
        file.extend(&jpeg[2..]);
        std::fs::write(&p, file).unwrap();
        let out = decode(image::ImageReader::open(&p).unwrap(), 512).unwrap();
        assert_eq!((out.width(), out.height()), (20, 40));
    }

    #[test]
    fn eviction_frees_least_recently_used_textures() {
        let ctx = egui::Context::default();
        let mut st = State::default();
        let img = egui::ColorImage::filled([16, 16], egui::Color32::RED);
        for i in 0..10u64 {
            let handle = ctx.load_texture(format!("t{i}"), img.clone(), Default::default());
            st.tex.insert(
                ThumbKey {
                    origin: String::new(),
                    path: format!("/{i}"),
                    mtime: 0,
                    size: 1,
                    px: 64,
                },
                Tex {
                    handle,
                    bytes: 1000,
                    last_frame: i,
                    last_used: Instant::now(),
                },
            );
            st.bytes += 1000;
        }
        evict(&mut st, 10, 4000, TEXTURE_IDLE);
        assert_eq!(st.bytes, 4000);
        let mut left: Vec<u64> = st.tex.values().map(|t| t.last_frame).collect();
        left.sort();
        assert_eq!(left, vec![6, 7, 8, 9], "the most recently drawn stay");
    }
}
