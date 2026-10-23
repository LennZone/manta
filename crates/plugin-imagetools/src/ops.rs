//! Operations: options, output naming, command builders, the lossless
//! metadata stripper and data URIs. Commands are argument lists (never
//! shell text), so file names can't break them; [`shell_line`] renders one
//! for the log.
//!
//! Resize, convert, rotate/flip, optimize and @2x keep using macOS's
//! `sips` (WebP: `cwebp`): it keeps colour profiles and colour-manages
//! (`-m` to sRGB), writes HEIC/TIFF, and its JPEG encoder is the system's.
//! The `image` crate would drop ICC profiles and EXIF orientation and
//! can't write HEIC, so it isn't a lossless-equivalent replacement.
//! Stripping metadata is done here, byte for byte ([`strip_bytes`]),
//! because sips copies EXIF (GPS included) into every file it writes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::headers::{self, Info, Tv};
use crate::i18n::tr;
use crate::meta;

pub const SRGB_PROFILE: &str = "/System/Library/ColorSync/Profiles/sRGB Profile.icc";

/// (id, title, kind, value): kind "scale" (factor) or "edge" (px).
pub const RESIZE_PRESETS: [(&str, &str, &str, f64); 5] = [
    ("50", "50%", "scale", 0.5),
    ("1920", "1920 px", "edge", 1920.0),
    ("1280", "1280 px", "edge", 1280.0),
    ("800", "800 px", "edge", 800.0),
    ("400", "400 px", "edge", 400.0),
];

/// (id, title, extension, sips format; `None` = cwebp).
pub const CONVERT_FORMATS: [(&str, &str, &str, Option<&str>); 5] = [
    ("jpeg", "JPEG", "jpg", Some("jpeg")),
    ("png", "PNG", "png", Some("png")),
    ("heic", "HEIC", "heic", Some("heic")),
    ("tiff", "TIFF", "tiff", Some("tiff")),
    ("webp", "WebP", "webp", None),
];

pub const QUALITIES: [u32; 4] = [60, 82, 90, 100];

/// The operations: (id, default suffix, default subfolder).
pub const OPS: [(&str, &str, &str); 6] = [
    ("resize", "-resized", "resized"),
    ("convert", "", "converted"),
    ("rotate", "-rotated", "rotated"),
    ("strip", "-clean", "clean"),
    ("optimize", "-web", "web"),
    ("retina", "", ""),
];

/// (id, sips args).
pub const TRANSFORMS: [(&str, [&str; 2]); 5] = [
    ("left", ["-r", "270"]),
    ("right", ["-r", "90"]),
    ("180", ["-r", "180"]),
    ("flip-h", ["-f", "horizontal"]),
    ("flip-v", ["-f", "vertical"]),
];

pub const WEB_EDGE: u32 = 2560;
pub const WEB_QUALITY: u32 = 82;
pub const DATA_URI_MAX: u64 = 100 * 1000;
pub const DATA_URI_WARN: u64 = 10 * 1000;

/// One operation's options (remembered in `options.json`, same format as
/// the Python plugin's).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub op: String,
    /// Resize: a preset id or "custom".
    pub preset: String,
    /// Resize custom: "width" or "height".
    pub custom_axis: String,
    pub custom_value: u32,
    /// Convert: a format id.
    pub format: String,
    /// Convert to JPEG/WebP/HEIC.
    pub quality: u32,
    /// Rotate: a transform id.
    pub transform: String,
    /// "suffix", "subfolder" or "overwrite".
    pub output: String,
    pub suffix: String,
    pub subfolder: String,
}

impl Default for Options {
    fn default() -> Self {
        Options::for_op("resize")
    }
}

impl Options {
    pub fn for_op(op: &str) -> Options {
        let (_, suffix, sub) = OPS
            .iter()
            .find(|o| o.0 == op)
            .copied()
            .unwrap_or(("", "", ""));
        Options {
            op: op.into(),
            preset: "1920".into(),
            custom_axis: "width".into(),
            custom_value: 1200,
            format: "jpeg".into(),
            quality: 82,
            transform: "right".into(),
            output: "suffix".into(),
            suffix: suffix.into(),
            subfolder: sub.into(),
        }
    }
}

/// `options.json` → remembered options per known operation.
pub fn parse_options(text: &str) -> HashMap<String, Options> {
    let mut out = HashMap::new();
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text) else {
        return out;
    };
    for (op, v) in map {
        if !OPS.iter().any(|o| o.0 == op) || !v.is_object() {
            continue;
        }
        let mut merged = serde_json::to_value(Options::for_op(&op)).unwrap_or_default();
        if let (Some(m), Some(o)) = (merged.as_object_mut(), v.as_object()) {
            for (k, val) in o {
                if m.contains_key(k) {
                    m.insert(k.clone(), val.clone());
                }
            }
        }
        if let Ok(mut o) = serde_json::from_value::<Options>(merged) {
            o.op = op.clone();
            out.insert(op, o);
        }
    }
    out
}

pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

/// An argument list as one shell-quoted line (for the log).
pub fn shell_line(argv: &[String]) -> String {
    argv.iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

// ------------------------------------------------------------------ sizes

/// (new w, new h) of a resize, or `None` when the image already fits
/// (presets never upscale; custom sizes may).
pub fn target_size(w: u32, h: u32, o: &Options) -> Option<(u32, u32)> {
    let r = |v: f64| (v.round() as u32).max(1);
    if o.preset == "custom" {
        let v = o.custom_value.max(1);
        return Some(if o.custom_axis == "width" {
            (v, r(h as f64 * v as f64 / w as f64))
        } else {
            (r(w as f64 * v as f64 / h as f64), v)
        });
    }
    let (_, _, kind, value) = RESIZE_PRESETS.iter().find(|p| p.0 == o.preset)?;
    if *kind == "scale" {
        return Some((r(w as f64 * value), r(h as f64 * value)));
    }
    if w.max(h) as f64 <= *value {
        return None;
    }
    let v = *value as u32;
    Some(if w >= h {
        (v, r(h as f64 * value / w as f64))
    } else {
        (r(w as f64 * value / h as f64), v)
    })
}

pub fn format_spec(
    id: &str,
) -> Option<(
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
)> {
    CONVERT_FORMATS.iter().find(|f| f.0 == id).copied()
}

/// (stem, ".ext") of a file name, like Python's `os.path.splitext`.
pub fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && !name[..i].chars().all(|c| c == '.') => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

fn parts(src: &Path) -> (PathBuf, String, String) {
    let folder = src.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, ext) = split_ext(&name);
    (folder, stem.to_string(), ext.to_string())
}

/// The extension a job writes: convert changes it, the rest keep the
/// source's.
pub fn convert_ext(o: &Options, src: &Path) -> String {
    if o.op == "convert" {
        if let Some(f) = format_spec(&o.format) {
            return format!(".{}", f.2);
        }
    }
    parts(src).2
}

// ------------------------------------------------------------------ output paths

fn lexists(p: &Path) -> bool {
    p.symlink_metadata().is_ok()
}

/// `path`, or "name-2.ext", "name-3.ext"… if it exists or is taken.
pub fn unique(path: &Path, taken: &HashSet<PathBuf>) -> PathBuf {
    if !lexists(path) && !taken.contains(path) {
        return path.to_path_buf();
    }
    let (folder, stem, ext) = parts(path);
    (2..)
        .map(|i| folder.join(format!("{stem}-{i}{ext}")))
        .find(|p| !lexists(p) && !taken.contains(p))
        .unwrap_or_else(|| path.to_path_buf())
}

/// Where an operation on `src` writes:
/// - "suffix": next to the original, "photo-resized.jpg" (unique);
/// - "subfolder": "resized/photo.jpg" (unique inside the folder);
/// - "overwrite": the original itself (same extension), or the original's
///   name with the new extension (unique unless it is the original).
///
/// Nothing but the original is ever overwritten.
pub fn output_path(
    src: &Path,
    o: &Options,
    ext: Option<&str>,
    taken: &HashSet<PathBuf>,
) -> PathBuf {
    let (folder, stem, old_ext) = parts(src);
    let ext = ext.unwrap_or(&old_ext);
    match o.output.as_str() {
        "overwrite" => {
            let p = folder.join(format!("{stem}{ext}"));
            if p == src {
                p
            } else {
                unique(&p, taken)
            }
        }
        "subfolder" => {
            let sub = o.subfolder.trim_matches('/');
            let sub = if sub.is_empty() { "output" } else { sub };
            unique(&folder.join(sub).join(format!("{stem}{ext}")), taken)
        }
        _ => {
            let mut p = folder.join(format!("{stem}{}{ext}", o.suffix));
            if p == src {
                p = folder.join(format!("{stem}-copy{ext}"));
            }
            unique(&p, taken)
        }
    }
}

/// The output name shown in the sheet ("photo-resized.jpg", "web/photo.jpg").
pub fn output_preview(src: &Path, o: &Options, ext: Option<&str>) -> String {
    let p = output_path(src, o, ext, &HashSet::new());
    let folder = src.parent().unwrap_or(Path::new(""));
    p.strip_prefix(folder)
        .map(|r| r.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.to_string_lossy().into_owned())
}

/// (path of the @2x file, path of the @1x file, rename the original?).
/// "logo.png" → logo@2x.png (the original, renamed) and logo.png at half
/// size; "logo@2x.png" stays, plus logo.png at half size. `Err` (a message)
/// if the @2x name is taken by another file.
pub fn retina_paths(
    src: &Path,
    taken: &HashSet<PathBuf>,
) -> Result<(PathBuf, PathBuf, bool), String> {
    let (folder, stem, ext) = parts(src);
    if let Some(base) = stem.strip_suffix("@2x") {
        let mut one = folder.join(format!("{base}{ext}"));
        if lexists(&one) || taken.contains(&one) {
            one = unique(&folder.join(format!("{base}@1x{ext}")), taken);
        }
        return Ok((src.to_path_buf(), one, false));
    }
    let two = folder.join(format!("{stem}@2x{ext}"));
    if lexists(&two) || taken.contains(&two) {
        let name = two
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err(tr!("retina.exists", name = name));
    }
    Ok((two, src.to_path_buf(), true))
}

// ------------------------------------------------------------------ commands

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn p(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `argv` writing to `target` instead of `out` (only the output argument
/// changes, even when the input is the same path).
pub fn retarget(argv: &[String], out: &Path, target: &Path) -> Option<Vec<String>> {
    let (o, t) = (p(out), p(target));
    let i = argv.iter().rposition(|a| *a == o)?;
    let mut v = argv.to_vec();
    v[i] = t;
    Some(v)
}

pub fn sips(args: &[String], src: &Path, out: &Path) -> Vec<String> {
    let mut v = vec!["sips".to_string()];
    v.extend(args.iter().cloned());
    v.extend([p(src), "--out".into(), p(out)]);
    v
}

pub fn resize_cmd(src: &Path, out: &Path, (w, h): (u32, u32)) -> Vec<String> {
    sips(&["-z".into(), h.to_string(), w.to_string()], src, out)
}

pub fn convert_cmd(
    src: &Path,
    out: &Path,
    o: &Options,
    have_cwebp: bool,
) -> Result<Vec<String>, String> {
    let (id, _, _, sips_fmt) = format_spec(&o.format).ok_or_else(|| o.format.clone())?;
    let q = if o.quality > 0 {
        o.quality
    } else {
        WEB_QUALITY
    }
    .to_string();
    let Some(fmt) = sips_fmt else {
        if !have_cwebp {
            return Err(tr!("webp.needs_cwebp").into());
        }
        return Ok([
            s(&["cwebp", "-quiet", "-metadata", "icc", "-q"]),
            vec![q, p(src), "-o".into(), p(out)],
        ]
        .concat());
    };
    let mut args = s(&["-s", "format", fmt]);
    if id == "jpeg" || id == "heic" {
        args.extend(["-s".into(), "formatOptions".into(), q]);
    }
    Ok(sips(&args, src, out))
}

pub fn transform_cmd(src: &Path, out: &Path, transform: &str) -> Option<Vec<String>> {
    let (_, args) = TRANSFORMS.iter().find(|t| t.0 == transform)?;
    Some(sips(&s(args), src, out))
}

/// Longest edge ≤ 2560, colours matched to sRGB, JPEG quality 82 (PNG kept
/// as PNG when it has transparency).
pub fn optimize_cmd(
    src: &Path,
    out: &Path,
    info: Option<&Info>,
    keep_png: bool,
    srgb: bool,
) -> Vec<String> {
    let mut args = Vec::new();
    if info.is_some_and(|i| i.width.max(i.height) > WEB_EDGE) {
        args.extend(["-Z".into(), WEB_EDGE.to_string()]);
    }
    if srgb {
        args.extend(["-m".into(), SRGB_PROFILE.into()]);
    }
    if keep_png {
        args.extend(s(&["-s", "format", "png"]));
    } else {
        args.extend(s(&["-s", "format", "jpeg", "-s", "formatOptions"]));
        args.push(WEB_QUALITY.to_string());
    }
    sips(&args, src, out)
}

/// ".png" for images with transparency, else ".jpg".
pub fn optimize_ext(src: &Path, info: Option<&Info>) -> &'static str {
    let ext = meta::ext_of(&p(src));
    if matches!(ext.as_str(), "png" | "gif" | "webp") && info.is_some_and(|i| i.alpha == Some(true))
    {
        ".png"
    } else {
        ".jpg"
    }
}

/// Remove all metadata but keep the colour profile and orientation.
pub fn strip_cmd_exiftool(src: &Path, out: &Path) -> Vec<String> {
    let mut v = s(&[
        "exiftool",
        "-q",
        "-m",
        "-all=",
        "-tagsFromFile",
        "@",
        "-icc_profile",
        "-Orientation",
    ]);
    if out == src {
        v.extend(["-overwrite_original".into(), p(src)]);
    } else {
        v.extend(["-o".into(), p(out), p(src)]);
    }
    v
}

pub fn half_cmd(src: &Path, out: &Path, w: u32, h: u32) -> Vec<String> {
    let half = |v: u32| ((v as f64 / 2.0).round() as u32).max(1);
    resize_cmd(src, out, (half(w), half(h)))
}

// ------------------------------------------------------------------ lossless strip

/// Metadata-free copy of a JPEG, PNG or WebP file, without re-encoding.
///
/// JPEG: drops APP1 (EXIF/XMP), APP13 (IPTC), APP3-APP12/APP15 and COM;
/// keeps JFIF, the ICC profile (APP2) and Adobe (APP14). A rotating EXIF
/// orientation is written back as a tiny EXIF block so the photo doesn't
/// turn sideways. PNG: drops eXIf, tEXt, zTXt, iTXt and tIME. WebP: drops
/// EXIF and XMP chunks and clears their VP8X flags.
pub fn strip_bytes(data: &[u8], fmt: &str) -> Result<Vec<u8>, String> {
    match fmt {
        "JPEG" => strip_jpeg(data),
        "PNG" => strip_png(data),
        "WebP" => strip_webp(data),
        other => Err(tr!("strip.needs_exiftool", format = other)),
    }
}

fn broken(what: &str) -> String {
    format!("broken {what}")
}

fn strip_jpeg(data: &[u8]) -> Result<Vec<u8>, String> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return Err(broken("JPEG"));
    }
    let orientation = headers::probe_bytes(data, false).and_then(|i| i.orientation);
    let mut out = vec![0xFF, 0xD8];
    if let Some(o) = orientation.filter(|o| *o != 1) {
        let tiff = headers::build_exif(&[(0x0112, Tv::Short(o))], &[], None, false);
        let mut body = b"Exif\0\0".to_vec();
        body.extend(tiff);
        out.extend([0xFF, 0xE1]);
        out.extend(((body.len() + 2) as u16).to_be_bytes());
        out.extend(body);
    }
    let mut i = 2;
    let n = data.len();
    while i + 4 <= n {
        if data[i] != 0xFF {
            return Err(broken("JPEG"));
        }
        let marker = data[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0xDA || marker == 0xD9 {
            out.extend(&data[i..]);
            return Ok(out);
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            out.extend(&data[i..i + 2]);
            i += 2;
            continue;
        }
        let ln = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        let end = (i + 2 + ln).min(n);
        let drop = matches!(marker, 0xE1 | 0xED | 0xFE | 0xE3..=0xEC | 0xEF);
        if !drop {
            out.extend(&data[i..end]);
        }
        i += 2 + ln;
    }
    Err(broken("JPEG"))
}

fn strip_png(data: &[u8]) -> Result<Vec<u8>, String> {
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(broken("PNG"));
    }
    let mut out = data[..8].to_vec();
    let mut i = 8;
    while i + 8 <= data.len() {
        let n = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        let typ = &data[i + 4..i + 8];
        let end = i + 12 + n;
        if end > data.len() {
            return Err(broken("PNG"));
        }
        if ![&b"eXIf"[..], b"tEXt", b"zTXt", b"iTXt", b"tIME"].contains(&typ) {
            out.extend(&data[i..end]);
        }
        i = end;
        if typ == b"IEND" {
            break;
        }
    }
    Ok(out)
}

fn strip_webp(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return Err(broken("WebP"));
    }
    let mut chunks = Vec::new();
    let mut i = 12;
    while i + 8 <= data.len() {
        let typ = &data[i..i + 4];
        let n = u32::from_le_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]]) as usize;
        let end = i + 8 + n + (n & 1);
        let mut chunk = data[i..end.min(data.len())].to_vec();
        if typ == b"VP8X" && chunk.len() > 8 {
            chunk[8] &= !(0x08 | 0x04);
        }
        if typ != b"EXIF" && typ != b"XMP " {
            chunks.extend(chunk);
        }
        i = end;
    }
    let mut out = b"RIFF".to_vec();
    out.extend(((chunks.len() + 4) as u32).to_le_bytes());
    out.extend(b"WEBP");
    out.extend(chunks);
    Ok(out)
}

// ------------------------------------------------------------------ data URI

pub fn mime(fmt: &str) -> Option<&'static str> {
    Some(match fmt {
        "PNG" => "image/png",
        "JPEG" => "image/jpeg",
        "GIF" => "image/gif",
        "WebP" => "image/webp",
        "BMP" => "image/bmp",
        "SVG" => "image/svg+xml",
        "AVIF" => "image/avif",
        _ => return None,
    })
}

pub fn data_uri(data: &[u8], fmt: &str) -> Option<String> {
    use base64::Engine as _;
    Some(format!(
        "data:{};base64,{}",
        mime(fmt)?,
        base64::engine::general_purpose::STANDARD.encode(data)
    ))
}

/// Whether a file of this name and size can be copied as a data URI, else
/// why not.
pub fn data_uri_ok(name: &str, size: Option<u64>) -> Result<(), String> {
    let Some(size) = size else {
        return Err(tr!("uri.unreadable").into());
    };
    if size > DATA_URI_MAX {
        return Err(tr!(
            "uri.too_big",
            size = meta::human_size(size),
            max = meta::human_size(DATA_URI_MAX)
        ));
    }
    if !matches!(
        meta::ext_of(name).as_str(),
        "png" | "jpg" | "jpeg" | "jpe" | "gif" | "webp" | "avif" | "bmp"
    ) {
        return Err(tr!("uri.format").into());
    }
    Ok(())
}

// ------------------------------------------------------------------ jobs

/// What one step of a job does.
#[derive(Debug, Clone, PartialEq)]
pub enum Work {
    /// Run a tool.
    Cmd(Vec<String>),
    /// Strip with [`strip_bytes`].
    Strip,
    /// Make an @1x file next to the @2x one.
    Retina { two: PathBuf, rename: bool },
    /// Nothing to do (the note says why).
    Skip,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub src: PathBuf,
    pub out: Option<PathBuf>,
    pub work: Work,
    pub note: Option<String>,
}

/// The steps of a batch, or a message for the user. `infos` maps a source
/// to its header info; `tools` says which optional tools exist.
pub fn plan(
    op: &str,
    files: &[PathBuf],
    o: &Options,
    infos: &HashMap<PathBuf, Option<Info>>,
    have_cwebp: bool,
    have_exiftool: bool,
    srgb: bool,
) -> Result<Vec<Step>, String> {
    let mut steps: Vec<Step> = Vec::new();
    let mut taken: HashSet<PathBuf> = HashSet::new();
    for src in files {
        let info = infos.get(src).and_then(|i| i.as_ref());
        let ext = parts(src).2;
        let step = |out: PathBuf, work: Work, note: Option<String>| Step {
            src: src.clone(),
            out: Some(out),
            work,
            note,
        };
        let skip = |note: &str| Step {
            src: src.clone(),
            out: None,
            work: Work::Skip,
            note: Some(note.to_string()),
        };
        let step = match op {
            "resize" => {
                let Some(i) = info else {
                    steps.push(skip(tr!("skip.unreadable")));
                    continue;
                };
                let Some(size) = target_size(i.width, i.height, o) else {
                    steps.push(skip(tr!("skip.small")));
                    continue;
                };
                let out = output_path(src, o, Some(&ext), &taken);
                let note = format!(
                    "{} → {}",
                    meta::dims(i.width, i.height),
                    meta::dims(size.0, size.1)
                );
                step(
                    out.clone(),
                    Work::Cmd(resize_cmd(src, &out, size)),
                    Some(note),
                )
            }
            "convert" => {
                let out = output_path(src, o, Some(&convert_ext(o, src)), &taken);
                step(
                    out.clone(),
                    Work::Cmd(convert_cmd(src, &out, o, have_cwebp)?),
                    None,
                )
            }
            "rotate" => {
                let out = output_path(src, o, Some(&ext), &taken);
                let cmd =
                    transform_cmd(src, &out, &o.transform).ok_or_else(|| o.transform.clone())?;
                step(out, Work::Cmd(cmd), None)
            }
            "optimize" => {
                let new_ext = optimize_ext(src, info);
                let out = output_path(src, o, Some(new_ext), &taken);
                step(
                    out.clone(),
                    Work::Cmd(optimize_cmd(src, &out, info, new_ext == ".png", srgb)),
                    None,
                )
            }
            "strip" => {
                let out = output_path(src, o, Some(&ext), &taken);
                if have_exiftool {
                    step(out.clone(), Work::Cmd(strip_cmd_exiftool(src, &out)), None)
                } else {
                    let fmt = info
                        .map(|i| i.format.clone())
                        .or_else(|| meta::format_name(&meta::ext_of(&p(src))).map(String::from))
                        .unwrap_or_else(|| "?".into());
                    if !matches!(fmt.as_str(), "JPEG" | "PNG" | "WebP") {
                        return Err(tr!("strip.needs_exiftool", format = fmt));
                    }
                    step(out, Work::Strip, None)
                }
            }
            "retina" => {
                if info.is_none() {
                    steps.push(skip(tr!("skip.unreadable")));
                    continue;
                }
                let (two, one, rename) = retina_paths(src, &taken)?;
                taken.insert(two.clone());
                step(one, Work::Retina { two, rename }, None)
            }
            other => return Err(other.to_string()),
        };
        if let Some(out) = &step.out {
            taken.insert(out.clone());
        }
        steps.push(step);
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headers::fixtures as B;
    use manta_plugin::i18n::{with_lang, Lang};

    const NASTY: [&str; 10] = [
        "plain.png",
        "with space.png",
        "it's.png",
        "double\"quote.png",
        "$(touch PWNED).png",
        "`touch PWNED2`.png",
        "semi;colon&amp.png",
        "-rf.png",
        "ünïcødé ✓.png",
        "--out.png",
    ];

    fn opts(op: &str, f: impl FnOnce(&mut Options)) -> Options {
        let mut o = Options::for_op(op);
        f(&mut o);
        o
    }

    fn none() -> HashSet<PathBuf> {
        HashSet::new()
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn suffix_and_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("photo.jpg");
        touch(&src);
        let o = Options::for_op("resize");
        assert_eq!(
            output_path(&src, &o, None, &none()),
            tmp.path().join("photo-resized.jpg")
        );
        touch(&tmp.path().join("photo-resized.jpg"));
        assert_eq!(
            output_path(&src, &o, None, &none()),
            tmp.path().join("photo-resized-2.jpg")
        );
        let taken: HashSet<PathBuf> = [tmp.path().join("photo-resized-2.jpg")].into();
        assert_eq!(
            output_path(&src, &o, None, &taken),
            tmp.path().join("photo-resized-3.jpg")
        );
        assert_eq!(output_preview(&src, &o, None), "photo-resized-2.jpg");
    }

    #[test]
    fn subfolder() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("photo.jpg");
        touch(&src);
        let mut o = opts("optimize", |o| o.output = "subfolder".into());
        assert_eq!(
            output_path(&src, &o, Some(".jpg"), &none()),
            tmp.path().join("web/photo.jpg")
        );
        touch(&tmp.path().join("web/photo.jpg"));
        assert_eq!(
            output_path(&src, &o, Some(".jpg"), &none()),
            tmp.path().join("web/photo-2.jpg")
        );
        assert_eq!(
            output_preview(
                &src,
                &opts("resize", |o| o.output = "subfolder".into()),
                None
            ),
            "resized/photo.jpg"
        );
        o.subfolder = "/".into();
        assert_eq!(
            output_path(&src, &o, Some(".jpg"), &none()),
            tmp.path().join("output/photo.jpg")
        );
    }

    #[test]
    fn overwrite_never_hits_another_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("photo.jpg");
        touch(&src);
        let o = opts("resize", |o| o.output = "overwrite".into());
        assert_eq!(
            output_path(&src, &o, None, &none()),
            src,
            "the original itself"
        );
        let c = opts("convert", |o| o.output = "overwrite".into());
        assert_eq!(
            output_path(&src, &c, Some(".png"), &none()),
            tmp.path().join("photo.png")
        );
        touch(&tmp.path().join("photo.png"));
        assert_eq!(
            output_path(&src, &c, Some(".png"), &none()),
            tmp.path().join("photo-2.png"),
            "never overwrites another file"
        );
    }

    #[test]
    fn convert_names() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("photo.jpg");
        touch(&src);
        let c = opts("convert", |o| o.format = "webp".into());
        assert_eq!(
            output_path(&src, &c, Some(&convert_ext(&c, &src)), &none()),
            tmp.path().join("photo.webp")
        );
        let c = opts("convert", |o| o.format = "jpeg".into());
        assert_eq!(
            output_path(&src, &c, Some(&convert_ext(&c, &src)), &none()),
            tmp.path().join("photo-copy.jpg"),
            "same format, no suffix: not the original"
        );
    }

    #[test]
    fn retina() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        touch(&d.join("logo.png"));
        assert_eq!(
            retina_paths(&d.join("logo.png"), &none()),
            Ok((d.join("logo@2x.png"), d.join("logo.png"), true))
        );
        touch(&d.join("icon@2x.png"));
        assert_eq!(
            retina_paths(&d.join("icon@2x.png"), &none()),
            Ok((d.join("icon@2x.png"), d.join("icon.png"), false))
        );
        touch(&d.join("icon.png"));
        assert_eq!(
            retina_paths(&d.join("icon@2x.png"), &none()).unwrap().1,
            d.join("icon@1x.png")
        );
        assert!(
            retina_paths(&d.join("icon.png"), &none()).is_err(),
            "icon@2x.png exists"
        );
    }

    #[test]
    fn presets() {
        let t = |w, h, p: &str| target_size(w, h, &opts("resize", |o| o.preset = p.into()));
        assert_eq!(t(1920, 1080, "50"), Some((960, 540)));
        assert_eq!(t(4000, 3000, "1920"), Some((1920, 1440)));
        assert_eq!(t(3000, 4000, "1280"), Some((960, 1280)));
        assert_eq!(t(800, 600, "800"), None, "never upscales");
        assert_eq!(t(801, 3, "400"), Some((400, 1)));
        assert_eq!(t(10, 10, "huge"), None);
    }

    #[test]
    fn custom() {
        let c = |w, h, axis: &str, v| {
            target_size(
                w,
                h,
                &opts("resize", |o| {
                    o.preset = "custom".into();
                    o.custom_axis = axis.into();
                    o.custom_value = v;
                }),
            )
        };
        assert_eq!(c(1920, 1080, "width", 1200), Some((1200, 675)));
        assert_eq!(c(1920, 1080, "height", 540), Some((960, 540)));
        assert_eq!(
            c(100, 100, "width", 300),
            Some((300, 300)),
            "custom may enlarge"
        );
    }

    #[test]
    fn builders() {
        let (src, out) = (
            Path::new("/a b/it's.png"),
            Path::new("/a b/it's-resized.png"),
        );
        let sv = |v: &[&str]| s(v);
        assert_eq!(
            resize_cmd(src, out, (960, 540)),
            sv(&[
                "sips",
                "-z",
                "540",
                "960",
                "/a b/it's.png",
                "--out",
                "/a b/it's-resized.png"
            ])
        );
        let jpeg = opts("convert", |o| {
            o.format = "jpeg".into();
            o.quality = 60;
        });
        assert_eq!(
            convert_cmd(src, Path::new("/x.jpg"), &jpeg, false).unwrap(),
            sv(&[
                "sips",
                "-s",
                "format",
                "jpeg",
                "-s",
                "formatOptions",
                "60",
                "/a b/it's.png",
                "--out",
                "/x.jpg"
            ])
        );
        let png = opts("convert", |o| o.format = "png".into());
        assert_eq!(
            convert_cmd(src, Path::new("/x.png"), &png, false).unwrap(),
            sv(&[
                "sips",
                "-s",
                "format",
                "png",
                "/a b/it's.png",
                "--out",
                "/x.png"
            ])
        );
        let webp = opts("convert", |o| o.format = "webp".into());
        assert_eq!(
            convert_cmd(src, Path::new("/x.webp"), &webp, true).unwrap(),
            sv(&[
                "cwebp",
                "-quiet",
                "-metadata",
                "icc",
                "-q",
                "82",
                "/a b/it's.png",
                "-o",
                "/x.webp"
            ])
        );
        assert!(convert_cmd(src, Path::new("/x.webp"), &webp, false).is_err());
        assert_eq!(
            transform_cmd(src, out, "left").unwrap(),
            sv(&[
                "sips",
                "-r",
                "270",
                "/a b/it's.png",
                "--out",
                "/a b/it's-resized.png"
            ])
        );
        assert_eq!(
            transform_cmd(src, out, "flip-h").unwrap()[1..3],
            sv(&["-f", "horizontal"])
        );
        let big = Info::new("JPEG", 6000, 4000);
        let cmd = optimize_cmd(src, out, Some(&big), false, true);
        assert_eq!(cmd[1..3], sv(&["-Z", "2560"]));
        assert!(
            cmd.contains(&"formatOptions".to_string()) && cmd.contains(&SRGB_PROFILE.to_string())
        );
        assert!(
            !optimize_cmd(src, out, Some(&Info::new("PNG", 800, 600)), true, false)
                .contains(&"-Z".to_string())
        );
        let ex = strip_cmd_exiftool(src, out);
        assert_eq!(
            ex[ex.len() - 3..],
            sv(&["-o", "/a b/it's-resized.png", "/a b/it's.png"])
        );
        let ex = strip_cmd_exiftool(src, src);
        assert_eq!(
            ex[ex.len() - 2..],
            sv(&["-overwrite_original", "/a b/it's.png"])
        );
        let same = sv(&[
            "sips",
            "-r",
            "90",
            "/a b/it's.png",
            "--out",
            "/a b/it's.png",
        ]);
        assert_eq!(
            retarget(&same, src, Path::new("/tmp/t.png")).unwrap(),
            sv(&["sips", "-r", "90", "/a b/it's.png", "--out", "/tmp/t.png"]),
            "only the output changes"
        );
    }

    #[test]
    fn shell_line_quotes_every_name() {
        // The log line, run by a real shell, gives back the exact argv.
        for name in NASTY {
            let src = PathBuf::from(format!("/tmp/x/{name}"));
            let out = PathBuf::from(format!("/tmp/x/{name}.out.png"));
            let argv = resize_cmd(&src, &out, (10, 10));
            let script = format!(
                "for a in {}; do printf '%s\\0' \"$a\"; done",
                shell_line(&argv)
            );
            let got = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(&script)
                .output()
                .unwrap();
            let back: Vec<String> = got
                .stdout
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect();
            assert_eq!(back, argv, "{name}");
        }
    }

    #[test]
    fn plan_skips_and_names() {
        with_lang(Lang::En, || {
            let tmp = tempfile::tempdir().unwrap();
            let (a, b) = (tmp.path().join("a.png"), tmp.path().join("b.png"));
            touch(&a);
            touch(&b);
            let mut infos = HashMap::new();
            infos.insert(
                a.clone(),
                Some(Info {
                    alpha: Some(false),
                    ..Info::new("PNG", 4000, 3000)
                }),
            );
            infos.insert(b.clone(), Some(Info::new("PNG", 300, 200)));
            let steps = plan(
                "resize",
                &[a.clone(), b.clone()],
                &Options::for_op("resize"),
                &infos,
                false,
                false,
                false,
            )
            .unwrap();
            assert_eq!(
                steps[0].out.as_deref(),
                Some(tmp.path().join("a-resized.png").as_path())
            );
            let Work::Cmd(c) = &steps[0].work else {
                panic!()
            };
            assert_eq!(c[1..4], s(&["-z", "1440", "1920"]));
            assert_eq!(steps[0].note.as_deref(), Some("4000×3000 → 1920×1440"));
            assert_eq!(steps[1].work, Work::Skip);
            assert_eq!(steps[1].note.as_deref(), Some("already small enough"));

            let alpha: HashMap<_, _> = [(
                a.clone(),
                Some(Info {
                    alpha: Some(true),
                    ..Info::new("PNG", 10, 10)
                }),
            )]
            .into();
            let st = plan(
                "optimize",
                std::slice::from_ref(&a),
                &Options::for_op("optimize"),
                &alpha,
                false,
                false,
                false,
            )
            .unwrap();
            assert_eq!(
                st[0].out.as_ref().unwrap().file_name().unwrap(),
                "a-web.png",
                "transparency stays PNG"
            );
            let opaque: HashMap<_, _> = [(
                a.clone(),
                Some(Info {
                    alpha: Some(false),
                    ..Info::new("PNG", 10, 10)
                }),
            )]
            .into();
            let st = plan(
                "optimize",
                std::slice::from_ref(&a),
                &Options::for_op("optimize"),
                &opaque,
                false,
                false,
                false,
            )
            .unwrap();
            assert_eq!(
                st[0].out.as_ref().unwrap().file_name().unwrap(),
                "a-web.jpg"
            );

            // Two files that would get the same output name.
            let c = tmp.path().join("a.jpg");
            touch(&c);
            let o = opts("convert", |o| {
                o.format = "webp".into();
                o.suffix = String::new();
            });
            let st = plan(
                "convert",
                &[a.clone(), c],
                &o,
                &HashMap::new(),
                true,
                false,
                false,
            )
            .unwrap();
            let names: Vec<_> = st
                .iter()
                .map(|s| {
                    s.out
                        .as_ref()
                        .unwrap()
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            assert_eq!(names, vec!["a.webp", "a-2.webp"]);

            let heic = tmp.path().join("x.heic");
            touch(&heic);
            assert!(plan(
                "strip",
                &[heic],
                &Options::for_op("strip"),
                &HashMap::new(),
                false,
                false,
                false
            )
            .is_err());
            let st = plan(
                "strip",
                std::slice::from_ref(&a),
                &Options::for_op("strip"),
                &HashMap::new(),
                false,
                true,
                false,
            )
            .unwrap();
            assert!(matches!(&st[0].work, Work::Cmd(c) if c[0] == "exiftool"));
            let st = plan(
                "strip",
                std::slice::from_ref(&a),
                &Options::for_op("strip"),
                &HashMap::new(),
                false,
                false,
                false,
            )
            .unwrap();
            assert_eq!(st[0].work, Work::Strip);
        });
    }

    #[test]
    fn options_json_from_the_python_plugin_loads() {
        let o = parse_options(
            r#"{"resize": {"op": "resize", "preset": "800", "output": "subfolder", "bogus": 1}, "nope": {}, "convert": 3}"#,
        );
        assert_eq!(o.len(), 1);
        assert_eq!(
            (
                o["resize"].preset.as_str(),
                o["resize"].output.as_str(),
                o["resize"].suffix.as_str()
            ),
            ("800", "subfolder", "-resized")
        );
        let text = serde_json::to_string(&o).unwrap();
        assert_eq!(parse_options(&text), o);
    }

    #[test]
    fn strip_jpeg_keeps_pixels_icc_and_orientation() {
        let exif = B::exif_photo(Some(B::PARIS), 6);
        let data = B::jpeg(
            400,
            300,
            false,
            Some(72),
            Some(&exif),
            Some("Display P3"),
            false,
        );
        let clean = strip_bytes(&data, "JPEG").unwrap();
        let i = headers::probe_bytes(&clean, false).unwrap();
        assert_eq!(i.exif.gps, None);
        assert_eq!(i.exif.make, None);
        assert_eq!(i.orientation, Some(6), "orientation written back");
        assert_eq!(i.profile.as_deref(), Some("Display P3"));
        assert_eq!((i.width, i.height), (400, 300));
        assert!(!clean.windows(7).any(|w| w == b"comment"));
        let sos = data.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
        assert!(
            clean.ends_with(&data[sos..]),
            "scan data copied byte for byte"
        );
        let plain = strip_bytes(
            &B::jpeg(
                10,
                10,
                false,
                None,
                Some(&B::exif_photo(Some(B::PARIS), 1)),
                None,
                false,
            ),
            "JPEG",
        )
        .unwrap();
        assert!(
            headers::probe_bytes(&plain, false).unwrap().exif.is_empty(),
            "no EXIF at all when upright"
        );
    }

    #[test]
    fn strip_png_and_webp() {
        let png = B::png(
            20,
            10,
            false,
            &[
                (b"eXIf", B::exif_photo(Some(B::PARIS), 1)),
                (b"tEXt", b"Comment\0hello".to_vec()),
                B::iccp_chunk("Display P3"),
            ],
            0,
        );
        let clean = strip_bytes(&png, "PNG").unwrap();
        let i = headers::probe_bytes(&clean, false).unwrap();
        assert!(i.exif.is_empty());
        assert_eq!(i.profile.as_deref(), Some("Display P3"));
        assert!(!clean.windows(4).any(|w| w == b"tEXt"));
        // Pixels are byte for byte the same.
        assert_eq!(
            image::load_from_memory(&clean).unwrap().to_rgb8(),
            image::load_from_memory(&png).unwrap().to_rgb8()
        );
        let webp = B::webp_extended(
            30,
            20,
            false,
            0,
            Some("Display P3"),
            Some(B::exif_photo(Some(B::PARIS), 1)),
        );
        let clean = strip_bytes(&webp, "WebP").unwrap();
        let i = headers::probe_bytes(&clean, false).unwrap();
        assert!(i.exif.is_empty());
        assert_eq!(i.profile.as_deref(), Some("Display P3"));
        assert_eq!(
            u32::from_le_bytes(clean[4..8].try_into().unwrap()) as usize,
            clean.len() - 8,
            "RIFF size fixed"
        );
        assert_eq!(clean[20] & 0x08, 0, "EXIF flag cleared");
    }

    #[test]
    fn strip_refuses_other_formats() {
        assert!(strip_bytes(&B::gif(2, 2, 1, false), "GIF").is_err());
        assert!(strip_bytes(b"nope", "JPEG").is_err());
    }

    #[test]
    fn data_uris() {
        assert_eq!(
            data_uri(b"GIF89a", "GIF").unwrap(),
            "data:image/gif;base64,R0lGODlh"
        );
        assert!(data_uri(&B::png(1, 1, false, &[], 0), "PNG")
            .unwrap()
            .starts_with("data:image/png;base64,iVBORw0KGgo"));
        assert!(data_uri(b"x", "TIFF").is_none());
        with_lang(Lang::En, || {
            assert_eq!(data_uri_ok("a.png", Some(5_000)), Ok(()));
            assert_eq!(
                data_uri_ok("a.png", Some(150_000)),
                Err("150 KB is too big for a data URI (max 100 KB)".into())
            );
            assert_eq!(
                data_uri_ok("a.tiff", Some(5)),
                Err("browsers can't show this format".into())
            );
            assert_eq!(
                data_uri_ok("a.png", None),
                Err("can't read the file".into())
            );
        });
    }
}
