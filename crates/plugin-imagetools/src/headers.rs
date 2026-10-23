//! Fast image header parsing.
//!
//! [`probe`] reads only what it needs from the start of a file (JPEG:
//! segment by segment, skipping the compressed data) and returns an
//! [`Info`]. The container walk below covers what no general decoder
//! exposes cheaply: DPI (`pHYs`, JFIF), progressive/interlaced, APNG and
//! animated GIF/WebP frame counts, the colour profile's name. The EXIF
//! block it finds is parsed with `kamadak-exif` ([`parse_exif`]). Files the
//! walk can't read but the `image` crate can still get their dimensions
//! from `ImageReader::into_dimensions` (no pixel decode).
//!
//! Nothing here panics or errors for broken files: `probe` returns `None`.

use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// Never read more than this (GIF frame counting).
const READ_CAP: u64 = 16 * 1024 * 1024;

/// EXIF essentials (all optional).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Exif {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub software: Option<String>,
    /// "YYYY:MM:DD HH:MM:SS" (EXIF), or what mdls/exiftool said.
    pub taken: Option<String>,
    pub orientation: Option<u16>,
    /// Seconds.
    pub exposure_time: Option<f64>,
    pub f_number: Option<f64>,
    pub iso: Option<f64>,
    pub focal_length: Option<f64>,
    pub focal_length_35mm: Option<f64>,
    pub color_space: Option<u32>,
    pub x_resolution: Option<f64>,
    pub y_resolution: Option<f64>,
    pub resolution_unit: Option<u16>,
    /// (latitude, longitude) in signed degrees.
    pub gps: Option<(f64, f64)>,
}

impl Exif {
    pub fn is_empty(&self) -> bool {
        *self == Exif::default()
    }

    /// First non-empty value wins, in the order given.
    pub fn merge(sources: &[&Exif]) -> Exif {
        fn s(v: &[&Exif], f: impl Fn(&Exif) -> &Option<String>) -> Option<String> {
            v.iter()
                .find_map(|e| f(e).clone().filter(|x| !x.is_empty()))
        }
        fn n<T: Copy>(v: &[&Exif], f: impl Fn(&Exif) -> Option<T>) -> Option<T> {
            v.iter().find_map(|e| f(e))
        }
        Exif {
            make: s(sources, |e| &e.make),
            model: s(sources, |e| &e.model),
            lens: s(sources, |e| &e.lens),
            software: s(sources, |e| &e.software),
            taken: s(sources, |e| &e.taken),
            orientation: n(sources, |e| e.orientation),
            exposure_time: n(sources, |e| e.exposure_time),
            f_number: n(sources, |e| e.f_number),
            iso: n(sources, |e| e.iso),
            focal_length: n(sources, |e| e.focal_length),
            focal_length_35mm: n(sources, |e| e.focal_length_35mm),
            color_space: n(sources, |e| e.color_space),
            x_resolution: n(sources, |e| e.x_resolution),
            y_resolution: n(sources, |e| e.y_resolution),
            resolution_unit: n(sources, |e| e.resolution_unit),
            gps: n(sources, |e| e.gps),
        }
    }
}

/// What the header says about an image.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Info {
    /// "PNG", "JPEG", "GIF", "WebP", "BMP", "HEIC", "TIFF", …
    pub format: String,
    /// Pixels as stored.
    pub width: u32,
    pub height: u32,
    pub alpha: Option<bool>,
    /// APNG, animated GIF, animated WebP.
    pub animated: bool,
    /// Frame count when animated and counted.
    pub frames: Option<u32>,
    /// Name of the embedded colour profile, "sRGB" for a PNG sRGB chunk.
    pub profile: Option<String>,
    /// "RGB", "Gray", "CMYK", "Indexed".
    pub color: Option<String>,
    /// Bits per channel (per pixel for BMP).
    pub bit_depth: Option<u32>,
    pub dpi: Option<(f64, f64)>,
    /// JPEG: progressive; PNG: interlaced.
    pub progressive: bool,
    pub exif: Exif,
    /// EXIF orientation 1-8 (5-8 = rotated 90°).
    pub orientation: Option<u16>,
}

impl Info {
    pub fn new(format: &str, width: u32, height: u32) -> Info {
        Info {
            format: format.into(),
            width,
            height,
            ..Info::default()
        }
    }

    /// (width, height) as shown, honouring a rotating EXIF orientation.
    pub fn display_size(&self) -> (u32, u32) {
        if matches!(self.orientation, Some(5..=8)) {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

/// Format name from the first bytes, or `None`.
pub fn sniff(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("PNG")
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some("JPEG")
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("GIF")
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("WebP")
    } else if head.starts_with(b"BM") && head.len() >= 26 {
        Some("BMP")
    } else {
        None
    }
}

trait Src: Read + Seek {}
impl<T: Read + Seek> Src for T {}

fn read_n(f: &mut dyn Src, n: usize) -> Option<Vec<u8>> {
    let mut v = Vec::with_capacity(n.min(1 << 20));
    f.take(n as u64).read_to_end(&mut v).ok()?;
    Some(v)
}

fn u16be(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32be(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u16le(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn i32le(b: &[u8], o: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn parse(f: &mut dyn Src, fmt: &str, deep: bool) -> Option<Info> {
    match fmt {
        "PNG" => png(f),
        "JPEG" => jpeg(f),
        "GIF" => gif(f, deep),
        "WebP" => webp(f, deep),
        "BMP" => bmp(f),
        _ => None,
    }
}

/// Header info of an image file (see [`Info`]) or `None`. `deep` also
/// counts the frames of animated GIF/WebP files (reads more).
pub fn probe(path: &Path, deep: bool) -> Option<Info> {
    let mut f = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let head = read_n(&mut f, 64)?;
    let fmt = sniff(&head)?;
    f.seek(SeekFrom::Start(0)).ok()?;
    parse(&mut f, fmt, deep).or_else(|| {
        // The image crate may still know the size.
        let (w, h) = image::ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()?;
        Some(Info::new(fmt, w, h))
    })
}

/// Like [`probe`], for bytes in memory.
pub fn probe_bytes(data: &[u8], deep: bool) -> Option<Info> {
    let fmt = sniff(&data[..data.len().min(64)])?;
    parse(&mut Cursor::new(data), fmt, deep)
}

// ------------------------------------------------------------------ PNG

fn png(f: &mut dyn Src) -> Option<Info> {
    let data = read_n(f, 8 + 25)?;
    if data.get(12..16)? != b"IHDR" {
        return None;
    }
    let (w, h) = (u32be(&data, 16)?, u32be(&data, 20)?);
    let (depth, ctype, interlace) = (data[24], data[25], data[28]);
    let mut info = Info::new("PNG", w, h);
    info.bit_depth = Some(depth as u32);
    info.alpha = Some(matches!(ctype, 4 | 6));
    info.color = match ctype {
        0 | 4 => Some("Gray".into()),
        2 | 6 => Some("RGB".into()),
        3 => Some("Indexed".into()),
        _ => None,
    };
    info.progressive = interlace == 1;
    while let Some(hdr) = read_n(f, 8).filter(|h| h.len() == 8) {
        let n = u32be(&hdr, 0)?;
        let typ = &hdr[4..8];
        if typ == b"IDAT" || typ == b"IEND" || n > 8 * 1024 * 1024 {
            break;
        }
        if [b"pHYs", b"iCCP", b"acTL", b"sRGB", b"tRNS", b"eXIf"]
            .iter()
            .any(|t| typ == *t)
        {
            let body = read_n(f, n as usize)?;
            f.seek(SeekFrom::Current(4)).ok()?;
            match typ {
                b"pHYs" if body.len() >= 9 => {
                    let (px, py, unit) = (u32be(&body, 0)?, u32be(&body, 4)?, body[8]);
                    if unit == 1 && px > 0 && py > 0 {
                        info.dpi = Some((round1(px as f64 * 0.0254), round1(py as f64 * 0.0254)));
                    }
                }
                b"iCCP" => {
                    let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
                    let name = String::from_utf8_lossy(&body[..end]).into_owned();
                    info.profile = Some(if name.is_empty() {
                        "ICC profile".into()
                    } else {
                        name
                    });
                    if let Some(raw) = body.get(end + 2..).and_then(inflate) {
                        if let Some(desc) = icc_description(&raw) {
                            info.profile = Some(desc);
                        }
                    }
                }
                b"sRGB" => {
                    if info.profile.is_none() {
                        info.profile = Some("sRGB".into());
                    }
                }
                b"acTL" if body.len() >= 8 => {
                    let frames = u32be(&body, 0)?;
                    info.animated = frames > 1;
                    info.frames = Some(frames);
                }
                b"tRNS" => info.alpha = Some(true),
                b"eXIf" => apply_exif(&mut info, parse_exif(&body)),
                _ => {}
            }
        } else {
            f.seek(SeekFrom::Current(n as i64 + 4)).ok()?;
        }
    }
    Some(info)
}

/// zlib-inflate (PNG `iCCP`), at most 4 MB.
fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, 4 << 20).ok()
}

// ------------------------------------------------------------------ JPEG

const SOF: [u8; 13] = [
    0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF,
];

fn jpeg(f: &mut dyn Src) -> Option<Info> {
    read_n(f, 2)?;
    let mut info: Option<Info> = None;
    let mut exif = Exif::default();
    let mut icc: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut jfif_dpi = None;
    let mut progressive = false;
    let mut one = [0u8; 1];
    loop {
        if f.read(&mut one).ok()? == 0 {
            break;
        }
        if one[0] != 0xFF {
            continue; // padding or garbage between segments
        }
        let mut m = [0xFFu8];
        while m[0] == 0xFF {
            if f.read(&mut m).ok()? == 0 {
                return finish_jpeg(info, progressive, icc, jfif_dpi, exif);
            }
        }
        let marker = m[0];
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        let Some(ln) = read_n(f, 2).filter(|l| l.len() == 2) else {
            break;
        };
        let n = u16be(&ln, 0)? as i64 - 2;
        if n < 0 {
            break;
        }
        let n = n as usize;
        if SOF.contains(&marker) {
            let body = read_n(f, n)?;
            let (depth, h, w, comps) = (
                *body.first()?,
                u16be(&body, 1)?,
                u16be(&body, 3)?,
                *body.get(5)?,
            );
            let mut i = Info::new("JPEG", w as u32, h as u32);
            i.bit_depth = Some(depth as u32);
            i.alpha = Some(false);
            i.color = match comps {
                1 => Some("Gray".into()),
                3 => Some("RGB".into()),
                4 => Some("CMYK".into()),
                _ => None,
            };
            progressive = matches!(marker, 0xC2 | 0xC6 | 0xCA | 0xCE);
            info = Some(i);
        } else if marker == 0xE0 {
            let body = read_n(f, n)?;
            if body.starts_with(b"JFIF\0") && body.len() >= 12 {
                let (units, xd, yd) = (body[7], u16be(&body, 8)?, u16be(&body, 10)?);
                if units == 1 && xd > 0 && yd > 0 {
                    jfif_dpi = Some((xd as f64, yd as f64));
                } else if units == 2 && xd > 0 && yd > 0 {
                    jfif_dpi = Some((round1(xd as f64 * 2.54), round1(yd as f64 * 2.54)));
                }
            }
        } else if marker == 0xE1 {
            let body = read_n(f, n)?;
            if body.starts_with(b"Exif\0\0") && exif.is_empty() {
                exif = parse_exif(&body[6..]);
            }
        } else if marker == 0xE2 {
            let body = read_n(f, n)?;
            if body.starts_with(b"ICC_PROFILE\0") && body.len() >= 14 {
                icc.push((body[12], body[14..].to_vec()));
            }
        } else {
            f.seek(SeekFrom::Current(n as i64)).ok()?;
        }
    }
    finish_jpeg(info, progressive, icc, jfif_dpi, exif)
}

fn finish_jpeg(
    info: Option<Info>,
    progressive: bool,
    mut icc: Vec<(u8, Vec<u8>)>,
    jfif_dpi: Option<(f64, f64)>,
    exif: Exif,
) -> Option<Info> {
    let mut info = info?;
    info.progressive = progressive;
    if !icc.is_empty() {
        icc.sort_by_key(|(k, _)| *k);
        let all: Vec<u8> = icc.into_iter().flat_map(|(_, d)| d).collect();
        info.profile = Some(icc_description(&all).unwrap_or_else(|| "ICC profile".into()));
    }
    info.dpi = jfif_dpi;
    apply_exif(&mut info, exif);
    Some(info)
}

fn apply_exif(info: &mut Info, exif: Exif) {
    if exif.is_empty() {
        return;
    }
    if exif.orientation.is_some_and(|o| o != 0) {
        info.orientation = exif.orientation;
    }
    if info.dpi.is_none() {
        if let Some(x) = exif.x_resolution {
            let y = exif.y_resolution.unwrap_or(x);
            let (x, y) = if exif.resolution_unit == Some(3) {
                (x * 2.54, y * 2.54)
            } else {
                (x, y)
            };
            info.dpi = Some((round1(x), round1(y)));
        }
    }
    if info.profile.is_none() && exif.color_space == Some(1) {
        info.profile = Some("sRGB".into());
    }
    info.exif = exif;
}

// ------------------------------------------------------------------ GIF

fn gif(f: &mut dyn Src, deep: bool) -> Option<Info> {
    let data = read_n(f, 13)?;
    let (w, h, flags) = (u16le(&data, 6)?, u16le(&data, 8)?, *data.get(10)?);
    let mut info = Info::new("GIF", w as u32, h as u32);
    info.color = Some("Indexed".into());
    info.bit_depth = Some((flags & 0x07) as u32 + 1);
    info.alpha = Some(false);
    if flags & 0x80 != 0 {
        f.seek(SeekFrom::Current(3 * (2i64 << (flags & 0x07))))
            .ok()?;
    }
    let mut frames = 0u32;
    let limit = if deep { None } else { Some(2) };
    let mut read: u64 = 13;
    let byte = |f: &mut dyn Src, read: &mut u64| -> Option<u8> {
        let mut b = [0u8; 1];
        *read += 1;
        (f.read(&mut b).ok()? == 1).then_some(b[0])
    };
    fn skip_sub_blocks(f: &mut dyn Src, read: &mut u64) -> Option<()> {
        loop {
            let mut b = [0u8; 1];
            *read += 1;
            if f.read(&mut b).ok()? == 0 || b[0] == 0 {
                return Some(());
            }
            f.seek(SeekFrom::Current(b[0] as i64)).ok()?;
            *read += b[0] as u64;
        }
    }
    while read < READ_CAP {
        let Some(b) = byte(f, &mut read) else { break };
        match b {
            0x3B => break,
            0x21 => {
                let label = byte(f, &mut read);
                if label == Some(0xF9) {
                    let blk = read_n(f, 6)?;
                    read += 6;
                    if blk.len() == 6 && blk[1] & 0x01 != 0 {
                        info.alpha = Some(true);
                    }
                    if blk.len() == 6 && blk[5] != 0 {
                        skip_sub_blocks(f, &mut read)?;
                    }
                } else {
                    skip_sub_blocks(f, &mut read)?;
                }
            }
            0x2C => {
                let desc = read_n(f, 9)?;
                read += 9;
                if desc.len() < 9 {
                    break;
                }
                frames += 1;
                if limit.is_some_and(|l| frames >= l) {
                    break;
                }
                if desc[8] & 0x80 != 0 {
                    let n = 3 * (2i64 << (desc[8] & 0x07));
                    f.seek(SeekFrom::Current(n)).ok()?;
                    read += n as u64;
                }
                byte(f, &mut read); // LZW minimum code size
                skip_sub_blocks(f, &mut read)?;
            }
            _ => break,
        }
    }
    info.animated = frames > 1;
    info.frames = (deep && frames > 1).then_some(frames);
    Some(info)
}

// ------------------------------------------------------------------ WebP

fn webp(f: &mut dyn Src, deep: bool) -> Option<Info> {
    let head = read_n(f, 12)?;
    let riff_size = u32le(&head, 4)? as u64;
    let end = (riff_size + 8).min(READ_CAP);
    let mut pos: u64 = 12;
    let mut info: Option<Info> = None;
    let mut frames = 0u32;
    let mut icc: Option<Vec<u8>> = None;
    let mut exif = Exif::default();
    while pos + 8 <= end {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let Some(hdr) = read_n(f, 8).filter(|h| h.len() == 8) else {
            break;
        };
        let (typ, n) = (&hdr[..4], u32le(&hdr, 4)? as u64);
        match typ {
            b"VP8X" => {
                let body = read_n(f, 10)?;
                let flags = *body.first()?;
                let w = 1 + (body[4] as u32 | (body[5] as u32) << 8 | (body[6] as u32) << 16);
                let h = 1 + (body[7] as u32 | (body[8] as u32) << 8 | (body[9] as u32) << 16);
                let mut i = Info::new("WebP", w, h);
                i.alpha = Some(flags & 0x10 != 0);
                i.animated = flags & 0x02 != 0;
                let stop = !(deep && i.animated) && flags & 0x28 == 0;
                info = Some(i);
                if stop {
                    break; // no ICC / EXIF to look for
                }
            }
            b"VP8 " if info.is_none() => {
                let body = read_n(f, 10)?;
                if body.get(3..6)? != b"\x9d\x01\x2a" {
                    return None;
                }
                let mut i = Info::new(
                    "WebP",
                    (u16le(&body, 6)? & 0x3FFF) as u32,
                    (u16le(&body, 8)? & 0x3FFF) as u32,
                );
                i.alpha = Some(false);
                info = Some(i);
                break;
            }
            b"VP8L" if info.is_none() => {
                let body = read_n(f, 5)?;
                if *body.first()? != 0x2F {
                    return None;
                }
                let bits = u32le(&body, 1)?;
                let mut i = Info::new("WebP", (bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1);
                i.alpha = Some((bits >> 28) & 1 == 1);
                info = Some(i);
                break;
            }
            b"ICCP" => icc = read_n(f, n.min(1 << 20) as usize),
            b"EXIF" => {
                let body = read_n(f, n.min(1 << 20) as usize)?;
                let body = body.strip_prefix(b"Exif\0\0").unwrap_or(&body);
                exif = parse_exif(body);
            }
            b"ANMF" => frames += 1,
            _ => {}
        }
        pos += 8 + n + (n & 1);
    }
    let mut info = info?;
    info.color = Some("RGB".into());
    info.bit_depth = Some(8);
    if let Some(icc) = icc {
        info.profile = Some(icc_description(&icc).unwrap_or_else(|| "ICC profile".into()));
    }
    if deep && info.animated && frames > 0 {
        info.frames = Some(frames);
    }
    apply_exif(&mut info, exif);
    Some(info)
}

// ------------------------------------------------------------------ BMP

fn bmp(f: &mut dyn Src) -> Option<Info> {
    let data = read_n(f, 54)?;
    let dib = u32le(&data, 14)?;
    let (w, h, bpp, dpi) = if dib == 12 {
        (
            u16le(&data, 18)? as i64,
            u16le(&data, 20)? as i64,
            u16le(&data, 24)?,
            None,
        )
    } else {
        let (w, h, bpp) = (
            i32le(&data, 18)? as i64,
            i32le(&data, 22)? as i64,
            u16le(&data, 28)?,
        );
        let (xp, yp) = (i32le(&data, 38).unwrap_or(0), i32le(&data, 42).unwrap_or(0));
        let dpi =
            (xp > 0 && yp > 0).then(|| (round1(xp as f64 * 0.0254), round1(yp as f64 * 0.0254)));
        (w, h, bpp, dpi)
    };
    let mut info = Info::new("BMP", w.unsigned_abs() as u32, h.unsigned_abs() as u32);
    info.bit_depth = Some(bpp as u32);
    info.alpha = Some(bpp == 32);
    info.color = Some(if bpp <= 8 { "Indexed" } else { "RGB" }.into());
    info.dpi = dpi;
    Some(info)
}

// ------------------------------------------------------------------ ICC

/// The profile description (`desc` tag) of an ICC profile, or `None`.
pub fn icc_description(data: &[u8]) -> Option<String> {
    if data.len() < 132 {
        return None;
    }
    let count = u32be(data, 128)?;
    for i in 0..count.min(100) as usize {
        let at = 132 + 12 * i;
        let sig = data.get(at..at + 4)?;
        if sig != b"desc" {
            continue;
        }
        let (off, size) = (u32be(data, at + 4)? as usize, u32be(data, at + 8)? as usize);
        let tag = data.get(off..off.checked_add(size)?.min(data.len()))?;
        if tag.starts_with(b"desc") {
            let n = u32be(tag, 8)? as usize;
            let raw = tag.get(12..(12 + n).min(tag.len()))?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            let s: String = raw[..end].iter().map(|&b| b as char).collect();
            let s = s.trim().to_string();
            return (!s.is_empty()).then_some(s);
        }
        if tag.starts_with(b"mluc") {
            if u32be(tag, 8)? < 1 {
                return None;
            }
            let (ln, start) = (u32be(tag, 20)? as usize, u32be(tag, 24)? as usize);
            let raw = tag.get(start..(start + ln).min(tag.len()))?;
            let units: Vec<u16> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            let s = String::from_utf16_lossy(&units);
            let s = s.trim_matches(|c| c == '\0' || c == ' ').to_string();
            return (!s.is_empty()).then_some(s);
        }
        return None;
    }
    None
}

// ------------------------------------------------------------------ EXIF

/// EXIF essentials from a TIFF-structured EXIF blob (after "Exif\0\0"),
/// parsed with `kamadak-exif`. Broken blobs give what could be read (or
/// nothing).
pub fn parse_exif(tiff: &[u8]) -> Exif {
    use exif::{In, Tag, Value};
    if tiff.len() < 8 || !(tiff.starts_with(b"II") || tiff.starts_with(b"MM")) {
        return Exif::default();
    }
    let mut reader = exif::Reader::new();
    reader.continue_on_error(true);
    let parsed = match reader.read_raw(tiff.to_vec()) {
        Ok(e) => e,
        Err(e) => match e.distill_partial_result(|_| {}) {
            Ok(e) => e,
            Err(_) => return Exif::default(),
        },
    };
    let field = |t: Tag| parsed.get_field(t, In::PRIMARY).map(|f| &f.value);
    let text = |t: Tag| match field(t) {
        Some(Value::Ascii(v)) => {
            let s = String::from_utf8_lossy(v.first()?)
                .trim()
                .trim_end_matches('\0')
                .to_string();
            (!s.is_empty()).then_some(s)
        }
        _ => None,
    };
    let float = |t: Tag| match field(t)? {
        Value::Rational(v) => v.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
        Value::SRational(v) => v.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
        v => v.get_uint(0).map(f64::from),
    };
    let uint = |t: Tag| field(t)?.get_uint(0);
    let dms = |t: Tag| match field(t)? {
        Value::Rational(v) if v.len() == 3 && v.iter().all(|r| r.denom != 0) => {
            Some(v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0)
        }
        _ => None,
    };
    let mut gps = None;
    if let (Some(mut lat), Some(mut lon)) = (dms(Tag::GPSLatitude), dms(Tag::GPSLongitude)) {
        if text(Tag::GPSLatitudeRef).is_some_and(|r| r.to_uppercase().starts_with('S')) {
            lat = -lat;
        }
        if text(Tag::GPSLongitudeRef).is_some_and(|r| r.to_uppercase().starts_with('W')) {
            lon = -lon;
        }
        if !(lat == 0.0 && lon == 0.0) {
            let r6 = |v: f64| (v * 1e6).round() / 1e6;
            gps = Some((r6(lat), r6(lon)));
        }
    }
    Exif {
        make: text(Tag::Make),
        model: text(Tag::Model),
        lens: text(Tag::LensModel),
        software: text(Tag::Software),
        taken: text(Tag::DateTimeOriginal),
        orientation: uint(Tag::Orientation).map(|v| v as u16),
        exposure_time: float(Tag::ExposureTime),
        f_number: float(Tag::FNumber),
        iso: float(Tag::PhotographicSensitivity),
        focal_length: float(Tag::FocalLength),
        focal_length_35mm: float(Tag::FocalLengthIn35mmFilm),
        color_space: uint(Tag::ColorSpace),
        x_resolution: float(Tag::XResolution),
        y_resolution: float(Tag::YResolution),
        resolution_unit: uint(Tag::ResolutionUnit).map(|v| v as u16),
        gps,
    }
}

/// A value for [`build_exif`].
#[derive(Debug, Clone)]
pub enum Tv {
    Ascii(String),
    Short(u16),
    Long(u32),
    Rational(u32, u32),
    Rationals(Vec<(u32, u32)>),
}

/// A minimal EXIF blob (TIFF structure): IFD0 entries, Exif sub-IFD
/// entries and an optional GPS position. Used to write the orientation
/// back after stripping, and by tests and fixtures.
pub fn build_exif(
    ifd0: &[(u16, Tv)],
    exif: &[(u16, Tv)],
    gps: Option<(f64, f64)>,
    big_endian: bool,
) -> Vec<u8> {
    let w16 = |v: u16| {
        if big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let w32 = |v: u32| {
        if big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let dms = |v: f64| {
        let v = v.abs();
        let d = v.trunc();
        let m = ((v - d) * 60.0).trunc();
        let s = (((v - d) * 60.0 - m) * 60.0 * 10000.0).round();
        Tv::Rationals(vec![(d as u32, 1), (m as u32, 1), (s as u32, 10000)])
    };
    let gps_entries: Vec<(u16, Tv)> = match gps {
        Some((lat, lon)) => vec![
            (1, Tv::Ascii(if lat < 0.0 { "S" } else { "N" }.into())),
            (2, dms(lat)),
            (3, Tv::Ascii(if lon < 0.0 { "W" } else { "E" }.into())),
            (4, dms(lon)),
        ],
        None => Vec::new(),
    };
    let mut ifd0: Vec<(u16, Tv)> = ifd0.to_vec();
    if !exif.is_empty() {
        ifd0.push((0x8769, Tv::Long(0)));
    }
    if !gps_entries.is_empty() {
        ifd0.push((0x8825, Tv::Long(0)));
    }
    let mut order: Vec<Vec<(u16, Tv)>> = vec![ifd0];
    if !exif.is_empty() {
        order.push(exif.to_vec());
    }
    if !gps_entries.is_empty() {
        order.push(gps_entries);
    }
    let mut offsets = Vec::new();
    let mut pos = 8u32;
    for d in &order {
        offsets.push(pos);
        pos += 2 + 12 * d.len() as u32 + 4;
    }
    let data_start = pos;
    let has_exif = !exif.is_empty();
    let n = order.len();
    for (tag, v) in order[0].iter_mut() {
        if *tag == 0x8769 {
            *v = Tv::Long(offsets[1]);
        } else if *tag == 0x8825 {
            *v = Tv::Long(offsets[n - 1]);
        }
    }
    let _ = has_exif;
    let encode = |v: &Tv| -> (u16, u32, Vec<u8>) {
        match v {
            Tv::Long(x) => (4, 1, w32(*x).to_vec()),
            Tv::Ascii(s) => {
                let mut raw = s.as_bytes().to_vec();
                raw.push(0);
                (2, raw.len() as u32, raw)
            }
            Tv::Short(x) => (3, 1, w16(*x).to_vec()),
            Tv::Rational(a, b) => (5, 1, [w32(*a), w32(*b)].concat()),
            Tv::Rationals(v) => (
                5,
                v.len() as u32,
                v.iter()
                    .flat_map(|(a, b)| [w32(*a), w32(*b)].concat())
                    .collect(),
            ),
        }
    };
    let mut blob = Vec::new();
    let mut data = Vec::new();
    for d in &mut order {
        d.sort_by_key(|(t, _)| *t);
        blob.extend(w16(d.len() as u16));
        for (tag, v) in d.iter() {
            let (typ, count, raw) = encode(v);
            let val = if raw.len() <= 4 {
                let mut r = raw.clone();
                r.resize(4, 0);
                r
            } else {
                let off = w32(data_start + data.len() as u32).to_vec();
                data.extend(&raw);
                if data.len() & 1 == 1 {
                    data.push(0);
                }
                off
            };
            blob.extend(w16(*tag));
            blob.extend(w16(typ));
            blob.extend(w32(count));
            blob.extend(val);
        }
        blob.extend(w32(0));
    }
    let mut out = if big_endian {
        b"MM".to_vec()
    } else {
        b"II".to_vec()
    };
    out.extend(w16(42));
    out.extend(w32(8));
    out.extend(blob);
    out.extend(data);
    out
}

/// `jpeg` with an APP1 EXIF segment holding `tiff` right after SOI.
pub fn insert_jpeg_exif(jpeg: &[u8], tiff: &[u8]) -> Vec<u8> {
    let mut body = b"Exif\0\0".to_vec();
    body.extend(tiff);
    let mut out = jpeg[..2].to_vec();
    out.extend([0xFF, 0xE1]);
    out.extend(((body.len() + 2) as u16).to_be_bytes());
    out.extend(body);
    out.extend(&jpeg[2..]);
    out
}

/// Hand-built image files for tests and renders (only the headers have to
/// be right; pixel data is minimal).
#[doc(hidden)]
pub mod fixtures {
    use super::{build_exif, Tv};

    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    pub fn chunk(typ: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend(typ);
        out.extend(data);
        out.extend(crc32(&[typ, data].concat()).to_be_bytes());
        out
    }

    /// zlib "stored" blocks (no compression; valid for any decoder).
    pub fn zlib_stored(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = if data.is_empty() {
            vec![&[][..]]
        } else {
            data.chunks(65535).collect()
        };
        for (i, b) in blocks.iter().enumerate() {
            out.push(u8::from(i + 1 == blocks.len()));
            out.extend((b.len() as u16).to_le_bytes());
            out.extend((!(b.len() as u16)).to_le_bytes());
            out.extend(*b);
        }
        let (mut a, mut s) = (1u32, 0u32);
        for &x in data {
            a = (a + x as u32) % 65521;
            s = (s + a) % 65521;
        }
        out.extend(((s << 16) | a).to_be_bytes());
        out
    }

    /// A valid PNG (gradient pixels) with extra chunks before IDAT.
    pub fn png(w: u32, h: u32, alpha: bool, extra: &[(&[u8], Vec<u8>)], interlace: u8) -> Vec<u8> {
        let ctype = if alpha { 6 } else { 2 };
        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend(h.to_be_bytes());
        ihdr.extend([8, ctype, 0, 0, interlace]);
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        out.extend(chunk(b"IHDR", &ihdr));
        for (t, d) in extra {
            out.extend(chunk(t, d));
        }
        let bpp = if alpha { 4 } else { 3 };
        let row: Vec<u8> = (0..w * bpp).map(|x| (x * 7) as u8).collect();
        let mut raw = Vec::new();
        for _ in 0..h {
            raw.push(0);
            raw.extend(&row);
        }
        out.extend(chunk(b"IDAT", &zlib_stored(&raw)));
        out.extend(chunk(b"IEND", &[]));
        out
    }

    pub fn phys_dpi(dpi: f64) -> (&'static [u8], Vec<u8>) {
        let ppm = (dpi / 0.0254).round() as u32;
        let mut d = ppm.to_be_bytes().to_vec();
        d.extend(ppm.to_be_bytes());
        d.push(1);
        (b"pHYs", d)
    }

    /// A minimal ICC profile whose only tag is the description.
    pub fn icc(desc: &str, v4: bool) -> Vec<u8> {
        let tag = if v4 {
            let text: Vec<u8> = desc.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
            let mut t = b"mluc".to_vec();
            t.extend([0; 4]);
            t.extend(1u32.to_be_bytes());
            t.extend(12u32.to_be_bytes());
            t.extend(b"enUS");
            t.extend((text.len() as u32).to_be_bytes());
            t.extend(28u32.to_be_bytes());
            t.extend(text);
            t
        } else {
            let mut raw = desc.as_bytes().to_vec();
            raw.push(0);
            let mut t = b"desc".to_vec();
            t.extend([0; 4]);
            t.extend((raw.len() as u32).to_be_bytes());
            t.extend(raw);
            t.extend([0; 12]);
            t
        };
        let mut header = vec![0u8; 128];
        header[16..20].copy_from_slice(b"RGB ");
        header[36..40].copy_from_slice(b"acsp");
        let mut data = header;
        data.extend(1u32.to_be_bytes());
        data.extend(b"desc");
        data.extend((128u32 + 4 + 12).to_be_bytes());
        data.extend((tag.len() as u32).to_be_bytes());
        data.extend(tag);
        let n = data.len() as u32;
        data[..4].copy_from_slice(&n.to_be_bytes());
        data
    }

    pub fn iccp_chunk(desc: &str) -> (&'static [u8], Vec<u8>) {
        let mut d = b"profile\0\0".to_vec();
        d.extend(zlib_stored(&icc(desc, false)));
        (b"iCCP", d)
    }

    pub fn gif(w: u16, h: u16, frames: u32, transparent: bool) -> Vec<u8> {
        let mut out = b"GIF89a".to_vec();
        out.extend(w.to_le_bytes());
        out.extend(h.to_le_bytes());
        out.extend([0x80, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff]);
        if frames > 1 {
            out.extend(b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x00\x00\x00");
        }
        for _ in 0..frames {
            out.extend([
                0x21,
                0xf9,
                0x04,
                u8::from(transparent),
                0x0a,
                0x00,
                0x00,
                0x00,
            ]);
            out.push(0x2c);
            for v in [0u16, 0, w, h] {
                out.extend(v.to_le_bytes());
            }
            out.push(0);
            out.extend([0x02, 0x02, 0x44, 0x01, 0x00]);
        }
        out.push(0x3b);
        out
    }

    pub fn bmp(w: u32, h: u32, bpp: u16, top_down: bool, core: bool) -> Vec<u8> {
        let dib: Vec<u8> = if core {
            [
                12u32.to_le_bytes().to_vec(),
                (w as u16).to_le_bytes().to_vec(),
                (h as u16).to_le_bytes().to_vec(),
                1u16.to_le_bytes().to_vec(),
                bpp.to_le_bytes().to_vec(),
            ]
            .concat()
        } else {
            let ppm = (72.0f64 / 0.0254).round() as i32;
            let hh = if top_down { -(h as i32) } else { h as i32 };
            [
                40u32.to_le_bytes().to_vec(),
                (w as i32).to_le_bytes().to_vec(),
                hh.to_le_bytes().to_vec(),
                1u16.to_le_bytes().to_vec(),
                bpp.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                ppm.to_le_bytes().to_vec(),
                ppm.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
            ]
            .concat()
        };
        let row = (w * bpp as u32).div_ceil(32) * 4;
        let pixels = vec![0u8; (row * h) as usize];
        let off = 14 + dib.len() as u32;
        let mut out = b"BM".to_vec();
        out.extend((off + pixels.len() as u32).to_le_bytes());
        out.extend([0; 4]);
        out.extend(off.to_le_bytes());
        out.extend(dib);
        out.extend(pixels);
        out
    }

    fn riff(chunks: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        for (t, d) in chunks {
            body.extend(*t);
            body.extend((d.len() as u32).to_le_bytes());
            body.extend(d);
            if d.len() & 1 == 1 {
                body.push(0);
            }
        }
        let mut out = b"RIFF".to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    pub fn webp_lossy(w: u16, h: u16) -> Vec<u8> {
        let mut frame = vec![0, 0, 0, 0x9d, 0x01, 0x2a];
        frame.extend(w.to_le_bytes());
        frame.extend(h.to_le_bytes());
        frame.extend([0; 8]);
        riff(&[(b"VP8 ", frame)])
    }

    pub fn webp_lossless(w: u32, h: u32, alpha: bool) -> Vec<u8> {
        let bits = (w - 1) | ((h - 1) << 14) | (u32::from(alpha) << 28);
        let mut d = vec![0x2f];
        d.extend(bits.to_le_bytes());
        d.extend([0; 4]);
        riff(&[(b"VP8L", d)])
    }

    pub fn webp_extended(
        w: u32,
        h: u32,
        alpha: bool,
        frames: u32,
        icc_desc: Option<&str>,
        exif: Option<Vec<u8>>,
    ) -> Vec<u8> {
        let animated = frames > 0;
        let flags = (if alpha { 0x10 } else { 0 })
            | (if animated { 0x02 } else { 0 })
            | (if icc_desc.is_some() { 0x20 } else { 0 })
            | (if exif.is_some() { 0x08 } else { 0 });
        let mut vp8x = vec![flags, 0, 0, 0];
        vp8x.extend(&(w - 1).to_le_bytes()[..3]);
        vp8x.extend(&(h - 1).to_le_bytes()[..3]);
        let mut chunks: Vec<(&[u8], Vec<u8>)> = vec![(b"VP8X", vp8x)];
        if let Some(d) = icc_desc {
            chunks.push((b"ICCP", icc(d, false)));
        }
        if animated {
            chunks.push((b"ANIM", vec![0; 6]));
            for _ in 0..frames {
                chunks.push((b"ANMF", vec![0; 16]));
            }
        } else {
            let mut d = vec![0x2f];
            d.extend(((w - 1) | ((h - 1) << 14)).to_le_bytes());
            d.extend([0; 4]);
            chunks.push((b"VP8L", d));
        }
        if let Some(e) = exif {
            chunks.push((b"EXIF", e));
        }
        riff(&chunks)
    }

    /// Headers of a baseline/progressive JPEG (no real scan data).
    pub fn jpeg(
        w: u16,
        h: u16,
        progressive: bool,
        jfif_dpi: Option<u16>,
        exif: Option<&[u8]>,
        icc_desc: Option<&str>,
        gray: bool,
    ) -> Vec<u8> {
        let seg = |m: u8, body: &[u8]| {
            let mut s = vec![0xff, m];
            s.extend(((body.len() + 2) as u16).to_be_bytes());
            s.extend(body);
            s
        };
        let mut out = vec![0xff, 0xd8];
        if let Some(d) = jfif_dpi {
            let mut app0 = b"JFIF\0\x01\x01\x01".to_vec();
            app0.extend(d.to_be_bytes());
            app0.extend(d.to_be_bytes());
            app0.extend([0, 0]);
            out.extend(seg(0xe0, &app0));
        }
        if let Some(e) = exif {
            out.extend(seg(0xe1, &[b"Exif\0\0".as_slice(), e].concat()));
        }
        if let Some(d) = icc_desc {
            out.extend(seg(
                0xe2,
                &[b"ICC_PROFILE\0\x01\x01".as_slice(), &icc(d, false)].concat(),
            ));
        }
        out.extend(seg(0xfe, b"comment"));
        let comps: u8 = if gray { 1 } else { 3 };
        let mut sof = vec![8];
        sof.extend(h.to_be_bytes());
        sof.extend(w.to_be_bytes());
        sof.push(comps);
        for _ in 0..comps {
            sof.extend([1, 0x11, 0]);
        }
        out.extend(seg(if progressive { 0xc2 } else { 0xc0 }, &sof));
        let mut sos = vec![comps];
        for _ in 0..comps {
            sos.extend([1, 0]);
        }
        sos.extend([0, 0x3f, 0]);
        out.extend(seg(0xda, &sos));
        out.extend([0x12, 0x34, 0x56, 0xff, 0xd9]);
        out
    }

    /// EXIF of a photo: a Canon with lens, exposure and GPS.
    pub fn exif_photo(gps: Option<(f64, f64)>, orientation: u16) -> Vec<u8> {
        build_exif(
            &[
                (0x010F, Tv::Ascii("Canon".into())),
                (0x0110, Tv::Ascii("Canon EOS R6".into())),
                (0x0112, Tv::Short(orientation)),
                (0x011A, Tv::Rational(3_000_000, 10_000)),
                (0x011B, Tv::Rational(3_000_000, 10_000)),
                (0x0128, Tv::Short(2)),
            ],
            &[
                (0x829A, Tv::Rational(1, 250)),
                (0x829D, Tv::Rational(28_000, 10_000)),
                (0x8827, Tv::Short(400)),
                (0x9003, Tv::Ascii("2026:09:28 14:05:00".into())),
                (0x920A, Tv::Rational(350_000, 10_000)),
                (0xA434, Tv::Ascii("RF35mm F1.8 MACRO IS STM".into())),
                (0xA001, Tv::Short(1)),
            ],
            gps,
            false,
        )
    }

    pub const PARIS: (f64, f64) = (48.858370, 2.294481);
}

#[cfg(test)]
mod tests {
    use super::fixtures as B;
    use super::*;

    fn p(d: &[u8]) -> Info {
        probe_bytes(d, false).expect("parses")
    }

    #[test]
    fn png_size_alpha_dpi_profile() {
        let i = p(&B::png(
            640,
            480,
            true,
            &[B::phys_dpi(144.0), B::iccp_chunk("Display P3")],
            0,
        ));
        assert_eq!((i.format.as_str(), i.width, i.height), ("PNG", 640, 480));
        assert_eq!(i.alpha, Some(true));
        assert_eq!(i.dpi, Some((144.0, 144.0)));
        assert_eq!(i.profile.as_deref(), Some("Display P3"));
        assert_eq!(i.color.as_deref(), Some("RGB"));
        assert!(!i.animated);
    }

    #[test]
    fn png_srgb_trns_interlace_and_apng() {
        let i = p(&B::png(
            20,
            10,
            false,
            &[(b"sRGB", vec![0]), (b"tRNS", vec![0; 6])],
            1,
        ));
        assert_eq!(i.profile.as_deref(), Some("sRGB"));
        assert_eq!(i.alpha, Some(true), "tRNS means transparency");
        assert!(i.progressive);
        let apng = p(&B::png(
            20,
            10,
            false,
            &[(b"acTL", vec![0, 0, 0, 12, 0, 0, 0, 0])],
            0,
        ));
        assert!(apng.animated);
        assert_eq!(apng.frames, Some(12));
    }

    #[test]
    fn png_exif_chunk() {
        let i = p(&B::png(
            30,
            20,
            false,
            &[(b"eXIf", B::exif_photo(Some(B::PARIS), 6))],
            0,
        ));
        assert_eq!(i.orientation, Some(6));
        assert_eq!(i.display_size(), (20, 30));
        assert_eq!(i.exif.make.as_deref(), Some("Canon"));
    }

    #[test]
    fn png_is_readable_by_the_image_crate() {
        // The fixtures are real images (stored zlib), not just headers.
        let data = B::png(12, 7, true, &[B::iccp_chunk("Display P3")], 0);
        let img = image::load_from_memory(&data).unwrap();
        assert_eq!((img.width(), img.height()), (12, 7));
        assert_eq!(inflate(&B::zlib_stored(b"hello")).unwrap(), b"hello");
    }

    #[test]
    fn real_deflate_streams_inflate() {
        // A PNG written by the image crate (dynamic Huffman blocks) and its
        // iCCP-like zlib payload.
        let mut buf = Vec::new();
        let img =
            image::RgbImage::from_fn(64, 32, |x, y| image::Rgb([x as u8 * 3, y as u8 * 5, 100]));
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let i = p(&buf);
        assert_eq!((i.width, i.height), (64, 32));
        // Find IDAT and inflate it: 32 rows × (1 + 64 × 3) bytes.
        let at = buf.windows(4).position(|w| w == b"IDAT").unwrap();
        let n = u32be(&buf, at - 4).unwrap() as usize;
        let raw = inflate(&buf[at + 4..at + 4 + n]).unwrap();
        assert_eq!(raw.len(), 32 * (1 + 64 * 3));
    }

    #[test]
    fn truncated_and_foreign_files() {
        assert!(probe_bytes(&B::png(5, 5, false, &[], 0)[..20], false).is_none());
        assert!(probe_bytes(b"%PDF-1.7\n", false).is_none());
        assert!(probe_bytes(b"", false).is_none());
        assert!(probe(Path::new("/no/such/file.png"), false).is_none());
    }

    #[test]
    fn jpeg_baseline_with_jfif() {
        let i = p(&B::jpeg(1920, 1080, false, Some(96), None, None, false));
        assert_eq!((i.format.as_str(), i.width, i.height), ("JPEG", 1920, 1080));
        assert!(!i.progressive);
        assert_eq!(i.alpha, Some(false));
        assert_eq!(i.dpi, Some((96.0, 96.0)));
        assert_eq!(i.color.as_deref(), Some("RGB"));
    }

    #[test]
    fn jpeg_progressive_gray_icc() {
        let i = p(&B::jpeg(
            300,
            200,
            true,
            None,
            None,
            Some("Generic Gray Gamma 2.2 Profile"),
            true,
        ));
        assert!(i.progressive);
        assert_eq!(i.color.as_deref(), Some("Gray"));
        assert_eq!(i.profile.as_deref(), Some("Generic Gray Gamma 2.2 Profile"));
    }

    #[test]
    fn icc_v4_mluc() {
        assert_eq!(
            p(&B::jpeg(10, 10, false, None, None, None, false)).profile,
            None
        );
        assert_eq!(
            icc_description(&B::icc("Display P3", true)).as_deref(),
            Some("Display P3")
        );
        assert_eq!(
            icc_description(&B::icc("sRGB IEC61966-2.1", false)).as_deref(),
            Some("sRGB IEC61966-2.1")
        );
        assert_eq!(icc_description(b"short"), None);
    }

    #[test]
    fn jpeg_exif_essentials_and_gps() {
        let exif = B::exif_photo(Some((-33.856784, 151.215297)), 8);
        let i = p(&B::jpeg(4000, 3000, false, None, Some(&exif), None, false));
        let e = &i.exif;
        assert_eq!(e.model.as_deref(), Some("Canon EOS R6"));
        assert_eq!(e.lens.as_deref(), Some("RF35mm F1.8 MACRO IS STM"));
        assert!((e.exposure_time.unwrap() - 1.0 / 250.0).abs() < 1e-9);
        assert!((e.f_number.unwrap() - 2.8).abs() < 1e-9);
        assert_eq!(e.iso, Some(400.0));
        assert_eq!(e.taken.as_deref(), Some("2026:09:28 14:05:00"));
        let (lat, lon) = e.gps.unwrap();
        assert!(
            (lat + 33.856784).abs() < 1e-4 && (lon - 151.215297).abs() < 1e-4,
            "{lat} {lon}"
        );
        assert_eq!(i.orientation, Some(8));
        assert_eq!(i.display_size(), (3000, 4000));
        assert_eq!(
            i.dpi,
            Some((300.0, 300.0)),
            "EXIF resolution when there is no JFIF"
        );
        assert_eq!(i.profile.as_deref(), Some("sRGB"), "EXIF ColorSpace 1");
    }

    #[test]
    fn big_endian_exif() {
        let t = build_exif(
            &[
                (0x010F, Tv::Ascii("NIKON CORPORATION".into())),
                (0x0110, Tv::Ascii("NIKON Z 6".into())),
            ],
            &[(0x8827, Tv::Short(100))],
            None,
            true,
        );
        let e = parse_exif(&t);
        assert_eq!(e.make.as_deref(), Some("NIKON CORPORATION"));
        assert_eq!(e.model.as_deref(), Some("NIKON Z 6"));
        assert_eq!(e.iso, Some(100.0));
    }

    #[test]
    fn broken_exif_is_ignored() {
        assert!(parse_exif(b"II*\0\xff\xff\xff\xff").is_empty());
        assert!(parse_exif(b"junk").is_empty());
        let i = p(&B::jpeg(
            10,
            20,
            false,
            None,
            Some(b"MM\0*\0\0\0\x08\0\x05"),
            None,
            false,
        ));
        assert_eq!((i.width, i.height), (10, 20));
    }

    #[test]
    fn jpeg_without_frame_header() {
        assert!(probe_bytes(
            b"\xff\xd8\xff\xe0\x00\x10JFIF\0\x01\x01\x00\x00\x01\x00\x01\x00\x00\xff\xd9",
            false
        )
        .is_none());
    }

    #[test]
    fn gif_static_animated_transparent() {
        let i = p(&B::gif(120, 45, 1, false));
        assert_eq!((i.format.as_str(), i.width, i.height), ("GIF", 120, 45));
        assert!(!i.animated);
        assert_eq!(i.alpha, Some(false));
        let data = B::gif(64, 64, 5, true);
        let quick = p(&data);
        assert!(quick.animated);
        assert_eq!(
            quick.frames, None,
            "the quick probe stops at the second frame"
        );
        let deep = probe_bytes(&data, true).unwrap();
        assert_eq!(deep.frames, Some(5));
        assert_eq!(deep.alpha, Some(true));
    }

    #[test]
    fn webp_lossy_lossless_extended_animated() {
        let i = p(&B::webp_lossy(1600, 900));
        assert_eq!(
            (i.format.as_str(), i.width, i.height, i.alpha),
            ("WebP", 1600, 900, Some(false))
        );
        let i = p(&B::webp_lossless(333, 222, true));
        assert_eq!((i.width, i.height, i.alpha), (333, 222, Some(true)));
        let i = p(&B::webp_extended(
            5000,
            4000,
            true,
            0,
            Some("Display P3"),
            Some(B::exif_photo(None, 1)),
        ));
        assert_eq!((i.width, i.height, i.alpha), (5000, 4000, Some(true)));
        assert_eq!(i.profile.as_deref(), Some("Display P3"));
        assert_eq!(i.exif.model.as_deref(), Some("Canon EOS R6"));
        let data = B::webp_extended(400, 300, false, 7, None, None);
        assert!(p(&data).animated);
        assert_eq!(probe_bytes(&data, true).unwrap().frames, Some(7));
    }

    #[test]
    fn bmp_variants() {
        let i = p(&B::bmp(31, 17, 24, false, false));
        assert_eq!(
            (i.format.as_str(), i.width, i.height, i.alpha),
            ("BMP", 31, 17, Some(false))
        );
        assert_eq!(i.dpi, Some((72.0, 72.0)));
        let i = p(&B::bmp(8, 9, 32, true, false));
        assert_eq!((i.width, i.height, i.alpha), (8, 9, Some(true)));
        let i = p(&B::bmp(12, 7, 24, false, true));
        assert_eq!((i.width, i.height), (12, 7));
    }

    #[test]
    fn exif_merge_order() {
        let a = Exif {
            make: Some("A".into()),
            lens: Some(String::new()),
            ..Exif::default()
        };
        let b = Exif {
            make: Some("B".into()),
            lens: Some("L".into()),
            iso: Some(100.0),
            ..Exif::default()
        };
        let m = Exif::merge(&[&a, &b, &Exif::default()]);
        assert_eq!(
            (m.make.as_deref(), m.lens.as_deref(), m.iso),
            (Some("A"), Some("L"), Some(100.0))
        );
    }

    #[test]
    fn image_crate_fallback_for_odd_files() {
        // A PNG the walk refuses (no IHDR right after the signature) but the
        // image crate can't read either: None. A real file: dimensions.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.png");
        image::RgbImage::new(21, 13).save(&path).unwrap();
        let i = probe(&path, false).unwrap();
        assert_eq!((i.width, i.height), (21, 13));
    }
}
