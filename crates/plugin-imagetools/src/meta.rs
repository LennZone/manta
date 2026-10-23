//! Metadata: parsing `sips -g`, `mdls` and `exiftool -json` output,
//! formatting values for people, and the decoration thresholds. Pure
//! functions only (no processes), so everything is tested on captured
//! output.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use manta_plugin::{Badge, Decoration, Tone};

use crate::headers::{Exif, Info};
use crate::i18n::{dec, month, tr};

/// Extensions with a header parser.
pub const HEADER_EXTS: &[&str] = &["png", "jpg", "jpeg", "jpe", "gif", "webp", "bmp"];
/// No header parser: their size comes from `sips` (and mdls in the panel).
pub const SIPS_EXTS: &[&str] = &["heic", "heif", "tif", "tiff", "avif"];
/// Formats that end up on web pages: the "XL" badge applies to them.
pub const WEB_EXTS: &[&str] = &["png", "jpg", "jpeg", "jpe", "gif", "webp", "avif"];

/// Longest edge below this: "lo".
pub const LOW_RES_EDGE: u32 = 800;
/// Web images above this: "XL".
pub const XL_BYTES: u64 = 2_000_000;

pub fn ext_of(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

pub fn is_image(path: &str) -> bool {
    let e = ext_of(path);
    HEADER_EXTS.contains(&e.as_str()) || SIPS_EXTS.contains(&e.as_str())
}

pub fn is_web(path: &str) -> bool {
    WEB_EXTS.contains(&ext_of(path).as_str())
}

pub fn format_name(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "PNG",
        "jpg" | "jpeg" | "jpe" => "JPEG",
        "gif" => "GIF",
        "webp" => "WebP",
        "bmp" => "BMP",
        "heic" => "HEIC",
        "heif" => "HEIF",
        "tif" | "tiff" => "TIFF",
        "avif" => "AVIF",
        _ => return None,
    })
}

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

// ------------------------------------------------------------------ formatting

/// 1000-based like the Finder: 340 KB, 8.4 MB.
pub fn human_size(n: u64) -> String {
    if n < 1000 {
        return tr!("bytes", count = n);
    }
    let mut v = n as f64;
    for i in 0..4 {
        let unit = manta_plugin::i18n::size_unit(i);
        v /= 1000.0;
        if v < 1000.0 || i == 3 {
            if v >= 100.0 {
                return format!("{} {unit}", v.round() as u64);
            }
            return dec(&format!("{v:.1} {unit}").replace(".0 ", " "));
        }
    }
    n.to_string()
}

pub fn dims(w: u32, h: u32) -> String {
    format!("{w}×{h}")
}

const COMMON_RATIOS: [(u32, u32); 14] = [
    (1, 1),
    (4, 3),
    (3, 2),
    (16, 9),
    (16, 10),
    (21, 9),
    (5, 4),
    (2, 1),
    (3, 1),
    (9, 16),
    (3, 4),
    (2, 3),
    (4, 5),
    (10, 16),
];

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// "16:9", "3:2", "≈ 16:9" or "1.91:1".
pub fn aspect_ratio(w: u32, h: u32) -> String {
    if w == 0 || h == 0 {
        return "–".into();
    }
    let g = gcd(w, h);
    let (a, b) = (w / g, h / g);
    if a <= 32 && b <= 32 {
        return format!("{a}:{b}");
    }
    let r = w as f64 / h as f64;
    for (x, y) in COMMON_RATIOS {
        if (r - x as f64 / y as f64).abs() < 0.01 {
            return format!("≈ {x}:{y}");
        }
    }
    if r >= 1.0 {
        dec(&format!("{r:.2}:1"))
    } else {
        dec(&format!("1:{:.2}", 1.0 / r))
    }
}

/// A shorter colour profile name for tooltips: "sRGB", "Display P3".
pub fn short_profile(name: Option<&str>) -> Option<String> {
    let n = name?.trim();
    if n.is_empty() {
        return None;
    }
    let low = n.to_lowercase();
    Some(if low.starts_with("srgb") {
        "sRGB".into()
    } else if low.contains("display p3") {
        "Display P3".into()
    } else if low.contains("adobe rgb") {
        "Adobe RGB".into()
    } else if low.contains("generic gray") || low.starts_with("gray") {
        "Gray".into()
    } else if n.chars().count() <= 24 {
        n.to_string()
    } else {
        format!("{}…", n.chars().take(23).collect::<String>())
    })
}

/// Python's "%.{p}g": `p` significant digits, trailing zeros dropped.
fn sig(v: f64, p: i32) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let decimals = (p - 1 - v.abs().log10().floor() as i32).max(0) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// 1/250 s, 0.5 s → 1/2 s, 2 s.
pub fn exposure(seconds: Option<f64>) -> Option<String> {
    let s = seconds.filter(|s| *s > 0.0)?;
    if s >= 1.0 {
        return Some(dec(&format!("{s:.1} s").replace(".0 ", " ")));
    }
    let den = (1.0 / s).round();
    if den >= 1.0 && ((1.0 / den - s) / s).abs() < 0.05 {
        return Some(format!("1/{den} s"));
    }
    Some(dec(&format!("{} s", sig(s, 2))))
}

pub fn f_number(v: Option<f64>) -> Option<String> {
    let v = v.filter(|v| *v > 0.0)?;
    Some(if v < 10.0 {
        dec(&format!("f/{v:.1}").replace(".0", ""))
    } else {
        format!("f/{}", v.round())
    })
}

/// A number without a pointless ".0" ("26", "5.96").
fn plain(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        dec(&format!("{v}"))
    }
}

/// EXIF "2026:09:28 14:05:00" → "28 Sep 2026, 14:05".
pub fn exif_date(s: &str) -> String {
    use chrono::{DateTime, Datelike, NaiveDateTime, Timelike};
    let t = s.trim();
    let naive = NaiveDateTime::parse_from_str(t, "%Y:%m:%d %H:%M:%S")
        .or_else(|_| DateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S %z").map(|d| d.naive_local()))
        .or_else(|_| NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S"))
        .or_else(|_| DateTime::parse_from_str(t, "%Y:%m:%d %H:%M:%S%z").map(|d| d.naive_local()));
    match naive {
        Ok(d) => tr!(
            "date.exif",
            day = d.day(),
            month = month(d.month()),
            year = d.year(),
            time = format!("{:02}:{:02}", d.hour(), d.minute())
        ),
        Err(_) => t.to_string(),
    }
}

/// "Canon" + "Canon EOS R6" → "Canon EOS R6".
pub fn camera_name(make: Option<&str>, model: Option<&str>) -> Option<String> {
    let (make, model) = (make.unwrap_or("").trim(), model.unwrap_or("").trim());
    if model.is_empty() {
        return (!make.is_empty()).then(|| make.to_string());
    }
    let first = make.split_whitespace().next().unwrap_or("").to_lowercase();
    if !make.is_empty() && model.to_lowercase().starts_with(&first) {
        return Some(model.to_string());
    }
    Some(format!("{make} {model}").trim().to_string())
}

/// "48.85837°N, 2.29448°E", with the hemispheres as the language writes
/// them (German O for east, Chinese 北纬 … 东经, Russian с. ш. … в. д.).
pub fn gps_text(lat: f64, lon: f64) -> String {
    let ns = if lat >= 0.0 {
        tr!("gps.n")
    } else {
        tr!("gps.s")
    };
    let ew = if lon >= 0.0 {
        tr!("gps.e")
    } else {
        tr!("gps.w")
    };
    let (lat, lon) = (
        dec(&format!("{:.5}", lat.abs())),
        dec(&format!("{:.5}", lon.abs())),
    );
    tr!("gps.format", lat = lat, ns = ns, lon = lon, ew = ew)
}

pub fn maps_url(lat: f64, lon: f64) -> String {
    format!("https://maps.apple.com/?ll={lat:.6},{lon:.6}")
}

/// (width, height) as shown (EXIF rotation applied).
pub fn display_dims(info: &Info) -> (u32, u32) {
    info.display_size()
}

/// "1920×1080 · sRGB · 340 KB" (unknown parts left out).
pub fn tooltip(info: Option<&Info>, size: u64) -> String {
    let mut parts = Vec::new();
    if let Some(i) = info.filter(|i| i.width > 0) {
        let (w, h) = display_dims(i);
        parts.push(dims(w, h));
    }
    if let Some(p) = info.and_then(|i| short_profile(i.profile.as_deref())) {
        parts.push(p);
    }
    if info.is_some_and(|i| i.animated) {
        parts.push(tr!("tip.animated").into());
    }
    parts.push(human_size(size));
    parts.join(" · ")
}

fn small_on_purpose(stem: &str) -> bool {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)^(favicon|apple-touch-icon|android-chrome|mstile|safari-pinned-tab)|(^|[-_.])icons?([-_.@]|$)|@1x\b|-(16|32|48|64|96|128)(x\d+)?$",
        )
        .expect("regex")
    })
    .is_match(stem)
}

fn stem_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

/// The decoration of one image file, or `None` for other files.
pub fn decoration(name: &str, size: u64, info: Option<&Info>) -> Option<Decoration> {
    let ext = ext_of(name);
    if !is_image(name) {
        return None;
    }
    let mut tip = tooltip(info, size);
    let mut notes = Vec::new();
    let xl = WEB_EXTS.contains(&ext.as_str()) && size > XL_BYTES;
    let lo = info.filter(|i| i.width > 0).is_some_and(|i| {
        let (w, h) = display_dims(i);
        w.max(h) < LOW_RES_EDGE && !small_on_purpose(stem_of(name))
    });
    let mut badge = None;
    if xl {
        badge = Some(Badge {
            text: "XL".into(),
            tone: Tone::Danger,
        });
        notes.push(tr!("tip.xl", size = human_size(XL_BYTES)));
    }
    if lo {
        if !xl {
            badge = Some(Badge {
                text: "lo".into(),
                tone: Tone::Warning,
            });
        }
        notes.push(tr!("tip.lo", px = LOW_RES_EDGE));
    }
    if !notes.is_empty() {
        tip = format!("{tip}\n{}", notes.join("\n"));
    }
    Some(Decoration {
        badge,
        name_tone: None,
        dimmed: false,
        tooltip: Some(tip),
    })
}

// ------------------------------------------------------------------ sips -g

/// One value of `sips -g` output.
#[derive(Debug, Clone, PartialEq)]
pub enum SipsValue {
    Int(u32),
    Float(f64),
    Bool(bool),
    Text(String),
    Nil,
}

/// `sips -g key … file1 file2 …` output → {path: {key: value}}.
pub fn parse_sips(text: &str) -> HashMap<String, HashMap<String, SipsValue>> {
    let mut out: HashMap<String, HashMap<String, SipsValue>> = HashMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with("  ") {
            let Some(c) = &cur else { continue };
            let t = line.trim();
            let (key, val) = t.split_once(": ").unwrap_or((t, ""));
            let val = val.trim();
            let v = if val.is_empty() || val == "<nil>" {
                SipsValue::Nil
            } else {
                match key {
                    "pixelWidth" | "pixelHeight" => {
                        val.parse().map_or(SipsValue::Nil, SipsValue::Int)
                    }
                    "dpiWidth" | "dpiHeight" => {
                        val.parse().map_or(SipsValue::Nil, SipsValue::Float)
                    }
                    "hasAlpha" => SipsValue::Bool(val == "yes"),
                    _ => SipsValue::Text(val.to_string()),
                }
            };
            out.entry(c.clone()).or_default().insert(key.to_string(), v);
        } else if !line.starts_with(' ') {
            let c = line.trim().to_string();
            out.insert(c.clone(), HashMap::new());
            cur = Some(c);
        }
    }
    out
}

/// An [`Info`] from one file's [`parse_sips`] values.
pub fn info_from_sips(d: Option<&HashMap<String, SipsValue>>, ext: &str) -> Option<Info> {
    let d = d?;
    let int = |k: &str| match d.get(k) {
        Some(SipsValue::Int(v)) if *v > 0 => Some(*v),
        _ => None,
    };
    let float = |k: &str| match d.get(k) {
        Some(SipsValue::Float(v)) if *v > 0.0 => Some(*v),
        _ => None,
    };
    let text = |k: &str| match d.get(k) {
        Some(SipsValue::Text(v)) => Some(v.clone()),
        _ => None,
    };
    let (w, h) = (int("pixelWidth")?, int("pixelHeight")?);
    let fmt = format_name(ext)
        .map(String::from)
        .unwrap_or_else(|| ext.to_uppercase());
    let mut info = Info::new(&fmt, w, h);
    info.alpha = match d.get("hasAlpha") {
        Some(SipsValue::Bool(b)) => Some(*b),
        _ => None,
    };
    info.profile = text("profile");
    info.color = text("space").filter(|s| matches!(s.as_str(), "RGB" | "Gray" | "CMYK"));
    info.bit_depth = text("bitsPerSample").and_then(|s| s.parse().ok());
    if let (Some(x), Some(y)) = (float("dpiWidth"), float("dpiHeight")) {
        info.dpi = Some((x, y));
    }
    Some(info)
}

// ------------------------------------------------------------------ mdls

/// One value of `mdls` output.
#[derive(Debug, Clone, PartialEq)]
pub enum Md {
    Int(i64),
    Float(f64),
    Text(String),
    List(Vec<Md>),
}

impl Md {
    fn num(&self) -> Option<f64> {
        match self {
            Md::Int(v) => Some(*v as f64),
            Md::Float(v) => Some(*v),
            _ => None,
        }
    }
    fn text(&self) -> Option<String> {
        match self {
            Md::Text(s) => Some(s.clone()),
            Md::Int(v) => Some(v.to_string()),
            Md::Float(v) => Some(v.to_string()),
            Md::List(_) => None,
        }
    }
}

fn md_scalar(v: &str) -> Md {
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        return Md::Text(v[1..v.len() - 1].replace("\\\"", "\""));
    }
    if let Ok(i) = v.parse::<i64>() {
        return Md::Int(i);
    }
    if let Ok(f) = v.parse::<f64>() {
        return Md::Float(f);
    }
    Md::Text(v.to_string())
}

/// `mdls file` output → {key: value}; `(null)` is dropped.
pub fn parse_mdls(text: &str) -> HashMap<String, Md> {
    let mut out = HashMap::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        let Some((key, val)) = line.split_once(" = ") else {
            continue;
        };
        let (key, val) = (key.trim().to_string(), val.trim());
        if val == "(" {
            let mut items = Vec::new();
            while i < lines.len() && lines[i].trim() != ")" {
                items.push(md_scalar(lines[i].trim().trim_end_matches(',')));
                i += 1;
            }
            i += 1;
            out.insert(key, Md::List(items));
            continue;
        }
        if val == "(null)" {
            continue;
        }
        out.insert(key, md_scalar(val));
    }
    out
}

/// The EXIF essentials from parsed mdls output.
pub fn exif_from_mdls(md: &HashMap<String, Md>) -> Exif {
    let text = |k: &str| md.get(k).and_then(Md::text).filter(|s| !s.is_empty());
    let num = |k: &str| md.get(k).and_then(Md::num);
    let mut e = Exif {
        make: text("kMDItemAcquisitionMake"),
        model: text("kMDItemAcquisitionModel"),
        exposure_time: num("kMDItemExposureTimeSeconds"),
        f_number: num("kMDItemFNumber"),
        iso: num("kMDItemISOSpeed"),
        focal_length: num("kMDItemFocalLength"),
        focal_length_35mm: num("kMDItemFocalLength35mm"),
        lens: text("kMDItemLensModel"),
        taken: text("kMDItemContentCreationDate"),
        ..Exif::default()
    };
    if let (Some(lat), Some(lon)) = (num("kMDItemLatitude"), num("kMDItemLongitude")) {
        if lat != 0.0 || lon != 0.0 {
            let r6 = |v: f64| (v * 1e6).round() / 1e6;
            e.gps = Some((r6(lat), r6(lon)));
        }
    }
    e
}

pub fn info_from_mdls(md: &HashMap<String, Md>, ext: &str) -> Option<Info> {
    let int = |k: &str| match md.get(k) {
        Some(Md::Int(v)) if *v > 0 => Some(*v as u32),
        _ => None,
    };
    let (w, h) = (int("kMDItemPixelWidth")?, int("kMDItemPixelHeight")?);
    let fmt = format_name(ext)
        .map(String::from)
        .unwrap_or_else(|| ext.to_uppercase());
    let mut info = Info::new(&fmt, w, h);
    info.alpha = md
        .get("kMDItemHasAlphaChannel")
        .and_then(Md::num)
        .map(|v| v != 0.0);
    info.profile = md.get("kMDItemProfileName").and_then(Md::text);
    info.color = md.get("kMDItemColorSpace").and_then(Md::text);
    let dpi = |k: &str| md.get(k).and_then(Md::num).filter(|v| *v > 0.0);
    if let (Some(x), Some(y)) = (
        dpi("kMDItemResolutionWidthDPI"),
        dpi("kMDItemResolutionHeightDPI"),
    ) {
        info.dpi = Some((x, y));
    }
    info.exif = exif_from_mdls(md);
    Some(info)
}

// ------------------------------------------------------------------ exiftool

fn fraction(s: &str) -> Option<f64> {
    match s.split_once('/') {
        Some((a, b)) => {
            let (a, b): (f64, f64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
            (b != 0.0).then(|| a / b)
        }
        None => s.trim().parse().ok(),
    }
}

/// `exiftool -json -n …` output (one file) → EXIF essentials.
pub fn parse_exiftool(text: &str) -> Exif {
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str::<serde_json::Value>(text) else {
        return Exif::default();
    };
    let Some(d) = list.first().and_then(|v| v.as_object()) else {
        return Exif::default();
    };
    let text = |k: &str| match d.get(k)? {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    let num = |k: &str| match d.get(k)? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => fraction(s),
        _ => None,
    };
    let mut e = Exif {
        make: text("Make"),
        model: text("Model"),
        lens: text("LensModel").or_else(|| text("LensID")),
        exposure_time: num("ExposureTime"),
        f_number: num("FNumber"),
        iso: num("ISO"),
        focal_length: num("FocalLength"),
        focal_length_35mm: num("FocalLengthIn35mmFormat"),
        taken: text("DateTimeOriginal"),
        orientation: num("Orientation").map(|v| v as u16),
        software: text("Software"),
        ..Exif::default()
    };
    let n = |k: &str| d.get(k).and_then(|v| v.as_f64());
    if let (Some(lat), Some(lon)) = (n("GPSLatitude"), n("GPSLongitude")) {
        if lat != 0.0 || lon != 0.0 {
            let r6 = |v: f64| (v * 1e6).round() / 1e6;
            e.gps = Some((r6(lat), r6(lon)));
        }
    }
    e
}

/// Rows for the panel's Camera section (empty if nothing is known).
pub fn exif_rows(e: &Exif) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if let Some(cam) = camera_name(e.make.as_deref(), e.model.as_deref()) {
        rows.push((tr!("exif.camera").to_string(), cam));
    }
    if let Some(l) = &e.lens {
        rows.push((tr!("exif.lens").to_string(), l.clone()));
    }
    let shot: Vec<String> = [
        exposure(e.exposure_time),
        f_number(e.f_number),
        e.iso.map(|i| format!("ISO {}", i as i64)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !shot.is_empty() {
        rows.push((tr!("exif.exposure").to_string(), shot.join(" · ")));
    }
    if let Some(fl) = e.focal_length.filter(|v| *v > 0.0) {
        let mut s = dec(&format!("{fl:.1} mm").replace(".0 ", " "));
        if let Some(mm) = e.focal_length_35mm.filter(|v| *v > 0.0) {
            s = tr!("exif.equiv", fl = s, mm = plain(mm));
        }
        rows.push((tr!("exif.focal").to_string(), s));
    }
    if let Some(t) = &e.taken {
        rows.push((tr!("exif.taken").to_string(), exif_date(t)));
    }
    if rows.is_empty() {
        if let Some(s) = &e.software {
            rows.push((tr!("exif.software").to_string(), s.clone()));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_plugin::i18n::{with_lang, Lang};

    const SIPS: &str = "/Users/anna/Photos/a.heic
  pixelWidth: 5712
  pixelHeight: 4284
  profile: Display P3
  hasAlpha: no
  dpiWidth: 72.000
  dpiHeight: 72.000
  space: RGB
  bitsPerSample: 8
/Users/anna/Photos/b c.tiff
  pixelWidth: 400
  pixelHeight: 300
  profile: <nil>
  hasAlpha: yes
  dpiWidth: 300.000
  dpiHeight: 300.000
  space: RGB
  bitsPerSample: 16
";

    const MDLS: &str = r#"_kMDItemDisplayNameWithExtensions      = "IMG_0001.HEIC"
kMDItemAcquisitionMake                 = "Apple"
kMDItemAcquisitionModel                = "iPhone 15"
kMDItemAltitude                        = 78.68497474747475
kMDItemAperture                        = 1.356143809255609
kMDItemBitsPerSample                   = 24
kMDItemColorSpace                      = "RGB"
kMDItemContentCreationDate             = 2024-07-22 05:01:39 +0000
kMDItemContentType                     = "public.heic"
kMDItemContentTypeTree                 = (
    "public.heic",
    "public.heif-standard",
    "public.image"
)
kMDItemExposureTimeSeconds             = 0.01666666666666667
kMDItemFNumber                         = 1.6
kMDItemFocalLength                     = 5.96
kMDItemFocalLength35mm                 = 26
kMDItemHasAlphaChannel                 = 0
kMDItemISOSpeed                        = 250
kMDItemKind                            = "HEIF Image"
kMDItemLatitude                        = 47.376887
kMDItemLongitude                       = 8.541694
kMDItemPixelHeight                     = 4284
kMDItemPixelWidth                      = 5712
kMDItemProfileName                     = "Display P3"
kMDItemResolutionHeightDPI             = 72
kMDItemResolutionWidthDPI              = 72
kMDItemUserTags                        = (null)
"#;

    const MDLS_UNINDEXED: &str = "kMDItemFSContentChangeDate = 2026-10-20 18:50:21 +0000
kMDItemFSName              = \"a.jpg\"
kMDItemFSSize              = 9091
";

    const EXIFTOOL: &str = r#"[{
  "SourceFile": "photo.jpg",
  "Make": "FUJIFILM",
  "Model": "X-T5",
  "LensModel": "XF33mmF1.4 R LM WR",
  "ExposureTime": 0.004,
  "FNumber": 2,
  "ISO": 160,
  "FocalLength": 33,
  "FocalLengthIn35mmFormat": 50,
  "DateTimeOriginal": "2026:05:01 09:30:12",
  "Orientation": 1,
  "GPSLatitude": -22.951916,
  "GPSLongitude": -43.210487
}]"#;

    fn en<R>(f: impl FnOnce() -> R) -> R {
        with_lang(Lang::En, f)
    }

    fn rows(e: &Exif) -> HashMap<String, String> {
        en(|| exif_rows(e).into_iter().collect())
    }

    #[test]
    fn sips_parse_and_info() {
        let d = parse_sips(SIPS);
        assert_eq!(d.len(), 2);
        let a = &d["/Users/anna/Photos/a.heic"];
        assert_eq!(a["pixelWidth"], SipsValue::Int(5712));
        assert_eq!(a["hasAlpha"], SipsValue::Bool(false));
        let b = &d["/Users/anna/Photos/b c.tiff"];
        assert_eq!(b["profile"], SipsValue::Nil);
        assert_eq!(b["dpiWidth"], SipsValue::Float(300.0));
        let i = info_from_sips(d.get("/Users/anna/Photos/a.heic"), "heic").unwrap();
        assert_eq!(
            (
                i.format.as_str(),
                i.width,
                i.height,
                i.profile.as_deref(),
                i.dpi
            ),
            ("HEIC", 5712, 4284, Some("Display P3"), Some((72.0, 72.0)))
        );
        let i = info_from_sips(d.get("/Users/anna/Photos/b c.tiff"), "tiff").unwrap();
        assert_eq!(
            (i.format.as_str(), i.bit_depth, i.alpha),
            ("TIFF", Some(16), Some(true))
        );
        assert!(info_from_sips(Some(&HashMap::new()), "heic").is_none());
        assert!(info_from_sips(None, "heic").is_none());
        assert_eq!(parse_sips("Warning: x\n").len(), 1);
    }

    #[test]
    fn mdls_parse_exif_and_info() {
        let md = parse_mdls(MDLS);
        assert_eq!(md["kMDItemPixelWidth"], Md::Int(5712));
        assert_eq!(md["kMDItemAcquisitionModel"], Md::Text("iPhone 15".into()));
        assert_eq!(
            md["kMDItemContentTypeTree"],
            Md::List(vec![
                Md::Text("public.heic".into()),
                Md::Text("public.heif-standard".into()),
                Md::Text("public.image".into())
            ])
        );
        assert!(!md.contains_key("kMDItemUserTags"), "(null) is dropped");
        let e = exif_from_mdls(&md);
        assert_eq!(
            (e.make.as_deref(), e.model.as_deref(), e.iso),
            (Some("Apple"), Some("iPhone 15"), Some(250.0))
        );
        assert_eq!(e.gps, Some((47.376887, 8.541694)));
        assert_eq!(e.taken.as_deref(), Some("2024-07-22 05:01:39 +0000"));
        let i = info_from_mdls(&md, "heic").unwrap();
        assert_eq!(
            (i.width, i.height, i.profile.as_deref(), i.alpha),
            (5712, 4284, Some("Display P3"), Some(false))
        );
        let r = rows(&e);
        assert_eq!(r["Camera"], "Apple iPhone 15");
        assert_eq!(r["Exposure"], "1/60 s · f/1.6 · ISO 250");
        assert_eq!(r["Focal length"], "6 mm (26 mm equiv.)");
        assert_eq!(r["Taken"], "22 Jul 2024, 05:01");
        let md = parse_mdls(MDLS_UNINDEXED);
        assert!(info_from_mdls(&md, "heic").is_none());
        assert!(exif_from_mdls(&md).is_empty());
    }

    #[test]
    fn exiftool_parse() {
        let e = parse_exiftool(EXIFTOOL);
        assert_eq!(e.lens.as_deref(), Some("XF33mmF1.4 R LM WR"));
        assert_eq!(e.gps, Some((-22.951916, -43.210487)));
        let r = rows(&e);
        assert_eq!(r["Camera"], "FUJIFILM X-T5");
        assert_eq!(r["Exposure"], "1/250 s · f/2 · ISO 160");
        assert_eq!(r["Focal length"], "33 mm (50 mm equiv.)");
        assert!(parse_exiftool("not json").is_empty());
        assert!(parse_exiftool("[]").is_empty());
        assert_eq!(
            parse_exiftool(r#"[{"ExposureTime": "1/125"}]"#).exposure_time,
            Some(0.008)
        );
    }

    #[test]
    fn sizes() {
        en(|| {
            assert_eq!(human_size(0), "0 bytes");
            assert_eq!(human_size(1), "1 byte");
            assert_eq!(human_size(340_000), "340 KB");
            assert_eq!(human_size(8_400_000), "8.4 MB");
            assert_eq!(human_size(2_000_000), "2 MB");
            assert_eq!(human_size(12_345), "12.3 KB");
        });
        with_lang(Lang::De, || {
            assert_eq!(human_size(8_400_000), "8,4 MB");
            assert_eq!(exif_date("2026:09:28 14:05:00"), "28. Sept. 2026, 14:05");
            assert_eq!(gps_text(48.85837, 2.294481), "48,85837°N, 2,29448°O");
        });
    }

    #[test]
    fn aspect() {
        assert_eq!(aspect_ratio(1920, 1080), "16:9");
        assert_eq!(aspect_ratio(6000, 4000), "3:2");
        en(|| assert_eq!(aspect_ratio(1200, 628), "1.91:1"));
        assert_eq!(aspect_ratio(1366, 768), "≈ 16:9");
        assert_eq!(aspect_ratio(1080, 1920), "9:16");
        assert_eq!(aspect_ratio(100, 1000), "1:10");
        assert_eq!(aspect_ratio(0, 10), "–");
    }

    #[test]
    fn camera_values() {
        en(|| {
            assert_eq!(exposure(Some(1.0 / 250.0)).as_deref(), Some("1/250 s"));
            assert_eq!(exposure(Some(0.5)).as_deref(), Some("1/2 s"));
            assert_eq!(exposure(Some(2.0)).as_deref(), Some("2 s"));
            assert_eq!(exposure(Some(1.3)).as_deref(), Some("1.3 s"));
            assert_eq!(exposure(Some(0.3)).as_deref(), Some("0.3 s"));
            assert_eq!(exposure(Some(0.0)), None);
            assert_eq!(f_number(Some(2.8)).as_deref(), Some("f/2.8"));
            assert_eq!(f_number(Some(8.0)).as_deref(), Some("f/8"));
            assert_eq!(f_number(Some(16.0)).as_deref(), Some("f/16"));
            assert_eq!(exif_date("2026:09:28 14:05:00"), "28 Sep 2026, 14:05");
            assert_eq!(exif_date("yesterday"), "yesterday");
        });
        assert_eq!(
            camera_name(Some("Canon"), Some("Canon EOS R6")).as_deref(),
            Some("Canon EOS R6")
        );
        assert_eq!(
            camera_name(Some("NIKON CORPORATION"), Some("NIKON Z 6")).as_deref(),
            Some("NIKON Z 6")
        );
        assert_eq!(
            camera_name(Some("Apple"), Some("iPhone 15")).as_deref(),
            Some("Apple iPhone 15")
        );
        assert_eq!(camera_name(Some(""), Some("")), None);
    }

    #[test]
    fn gps() {
        en(|| {
            assert_eq!(gps_text(48.85837, 2.294481), "48.85837°N, 2.29448°E");
            assert_eq!(gps_text(-33.8568, -70.1), "33.85680°S, 70.10000°W");
        });
        let cases = [
            (Lang::Fr, "48,85837°N, 2,29448°E", "33,85680°S, 70,10000°O"),
            (Lang::Es, "48,85837°N, 2,29448°E", "33,85680°S, 70,10000°O"),
            (Lang::It, "48,85837°N, 2,29448°E", "33,85680°S, 70,10000°O"),
            (
                Lang::Ru,
                "48,85837°\u{a0}с.\u{a0}ш., 2,29448°\u{a0}в.\u{a0}д.",
                "33,85680°\u{a0}ю.\u{a0}ш., 70,10000°\u{a0}з.\u{a0}д.",
            ),
            (
                Lang::Zh,
                "北纬 48.85837°，东经 2.29448°",
                "南纬 33.85680°，西经 70.10000°",
            ),
            (Lang::De, "48,85837°N, 2,29448°O", "33,85680°S, 70,10000°W"),
        ];
        for (l, north_east, south_west) in cases {
            with_lang(l, || {
                assert_eq!(gps_text(48.85837, 2.294481), north_east, "{l:?}");
                assert_eq!(gps_text(-33.8568, -70.1), south_west, "{l:?}");
            });
        }
        assert_eq!(
            maps_url(-33.856784, 151.215297),
            "https://maps.apple.com/?ll=-33.856784,151.215297"
        );
    }

    fn info(w: u32, h: u32, orientation: Option<u16>, animated: bool) -> Info {
        Info {
            profile: Some("sRGB IEC61966-2.1".into()),
            orientation,
            animated,
            ..Info::new("PNG", w, h)
        }
    }

    fn badge(name: &str, size: u64, i: &Info) -> Option<String> {
        en(|| {
            decoration(name, size, Some(i))
                .and_then(|d| d.badge)
                .map(|b| b.text)
        })
    }

    #[test]
    fn profiles_and_tooltip() {
        assert_eq!(
            short_profile(Some("sRGB IEC61966-2.1")).as_deref(),
            Some("sRGB")
        );
        assert_eq!(
            short_profile(Some("Display P3")).as_deref(),
            Some("Display P3")
        );
        assert_eq!(
            short_profile(Some("Adobe RGB (1998)")).as_deref(),
            Some("Adobe RGB")
        );
        assert_eq!(short_profile(None), None);
        en(|| {
            assert_eq!(
                tooltip(Some(&info(1920, 1080, None, false)), 340_000),
                "1920×1080 · sRGB · 340 KB"
            );
            assert_eq!(tooltip(None, 340_000), "340 KB");
        });
    }

    #[test]
    fn low_resolution_edge() {
        assert_eq!(
            badge("a.jpg", 50_000, &info(799, 400, None, false)).as_deref(),
            Some("lo")
        );
        assert_eq!(
            badge("a.jpg", 50_000, &info(800, 400, None, false)),
            None,
            "800 px is fine"
        );
        assert_eq!(
            badge("a.jpg", 50_000, &info(300, 800, None, false)),
            None,
            "longest edge counts"
        );
        let d = en(|| decoration("a.jpg", 50_000, Some(&info(799, 400, None, false))).unwrap());
        assert_eq!(d.badge.unwrap().tone, Tone::Warning);
        assert!(d.tooltip.unwrap().contains("Low resolution"));
    }

    #[test]
    fn xl_edge_and_formats() {
        let big = info(3000, 2000, None, false);
        assert_eq!(badge("a.png", 2_000_000, &big), None);
        assert_eq!(badge("a.png", 2_000_001, &big).as_deref(), Some("XL"));
        for name in ["a.jpg", "a.JPEG", "a.webp", "a.gif", "a.avif"] {
            assert_eq!(
                badge(name, 3_000_000, &big).as_deref(),
                Some("XL"),
                "{name}"
            );
        }
        for name in ["a.tiff", "a.heic", "a.bmp"] {
            assert_eq!(
                badge(name, 30_000_000, &big),
                None,
                "not a web format: {name}"
            );
        }
    }

    #[test]
    fn both_show_xl_and_explain_both() {
        let d = en(|| decoration("a.gif", 5_000_000, Some(&info(400, 300, None, true))).unwrap());
        assert_eq!(d.badge.unwrap().text, "XL");
        let tip = d.tooltip.unwrap();
        assert!(tip.contains("Large for the web") && tip.contains("Low resolution"));
        assert!(tip.starts_with("400×300 · sRGB · animated · 5 MB"), "{tip}");
    }

    #[test]
    fn rotated_photos_use_the_shown_size() {
        let d =
            en(|| decoration("p.jpg", 100_000, Some(&info(1200, 700, Some(6), false))).unwrap());
        assert!(d.tooltip.unwrap().starts_with("700×1200"));
    }

    #[test]
    fn icons_are_small_on_purpose() {
        let small = info(64, 64, None, false);
        for name in [
            "favicon-32x32.png",
            "apple-touch-icon.png",
            "icon-64.png",
            "app_icon.png",
            "icons.png",
            "logo@1x.png",
            "android-chrome-192x192.png",
            "arrow-16.png",
        ] {
            assert_eq!(badge(name, 2_000, &small), None, "{name}");
        }
        assert_eq!(
            badge("silicone.png", 2_000, &small).as_deref(),
            Some("lo"),
            "'icon' inside a word"
        );
    }

    #[test]
    fn tooltip_only_and_unknowns() {
        let d = en(|| decoration("a.png", 340_000, Some(&info(1920, 1080, None, false))).unwrap());
        assert_eq!(
            (d.badge, d.tooltip.as_deref()),
            (None, Some("1920×1080 · sRGB · 340 KB"))
        );
        assert_eq!(
            en(|| decoration("a.heic", 340_000, None).unwrap().tooltip),
            Some("340 KB".into())
        );
        assert!(decoration("notes.txt", 10, None).is_none());
        assert!(decoration("vector.svg", 10, None).is_none());
    }
}
