//! Embeds the user manual (`docs/manual`) into the binary: every chapter of
//! every language, `index.json` and the screenshots in `images/<lang>/`.
//! Generates `$OUT_DIR/manual_files.rs` (used by `src/help/manual.rs`), so
//! the release binary is self-contained and new chapters or images are
//! picked up without code changes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The screenshots stay PNGs in the repository (GitHub shows them,
/// `render_manual_images` writes them) but are embedded as lossless WebP,
/// about a third smaller: made by `cwebp` (libwebp, `brew install webp`)
/// when it is installed, else the PNGs are embedded as they are. Release
/// builds set `MANTA_REQUIRE_CWEBP=1` (scripts/build-release.sh), so a
/// missing `cwebp` can't silently make the update bigger. `MANTA_CWEBP`
/// names the tool (`0`: no conversion).
mod webp {
    use std::hash::{Hash, Hasher};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Part of the cache key: changing them re-encodes everything.
    const OPTIONS: &[&str] = &["-quiet", "-lossless", "-z", "6", "-metadata", "none"];

    fn tool() -> Option<String> {
        println!("cargo:rerun-if-env-changed=MANTA_CWEBP");
        println!("cargo:rerun-if-env-changed=MANTA_REQUIRE_CWEBP");
        let required = std::env::var("MANTA_REQUIRE_CWEBP").is_ok_and(|v| v == "1");
        let name = std::env::var("MANTA_CWEBP").unwrap_or_else(|_| "cwebp".into());
        if name == "0" {
            assert!(!required, "MANTA_REQUIRE_CWEBP=1 but MANTA_CWEBP=0");
            return None;
        }
        let found = Command::new(&name)
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success());
        if found {
            return Some(name);
        }
        assert!(
            !required,
            "`{name}` not found: install libwebp (`brew install webp`) or unset MANTA_REQUIRE_CWEBP"
        );
        println!(
            "cargo:warning=cwebp not found: the manual's screenshots are embedded as PNG (bigger binary)"
        );
        None
    }

    /// The file to embed for each PNG: its WebP version (in `cache`) when
    /// that is smaller, else the PNG. Unchanged images are not re-encoded.
    pub fn compress_all(pngs: &[PathBuf], cache: &Path) -> Vec<PathBuf> {
        let Some(cwebp) = tool() else {
            return pngs.to_vec();
        };
        std::fs::create_dir_all(cache).expect("OUT_DIR is writable");
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let next = AtomicUsize::new(0);
        let out = Mutex::new(pngs.to_vec());
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(png) = pngs.get(i) else { break };
                    let file = compress(&cwebp, png, cache);
                    out.lock().unwrap()[i] = file;
                });
            }
        });
        out.into_inner().unwrap()
    }

    fn compress(cwebp: &str, png: &Path, cache: &Path) -> PathBuf {
        let bytes = std::fs::read(png).expect("readable PNG");
        let mut h = std::hash::DefaultHasher::new();
        bytes.hash(&mut h);
        OPTIONS.hash(&mut h);
        let webp = cache.join(format!("{:016x}.webp", h.finish()));
        if !webp.is_file() {
            let tmp = webp.with_extension("part");
            let ok = Command::new(cwebp)
                .args(OPTIONS)
                .arg(png)
                .arg("-o")
                .arg(&tmp)
                .status()
                .is_ok_and(|s| s.success());
            if !ok || std::fs::rename(&tmp, &webp).is_err() {
                println!(
                    "cargo:warning=cwebp failed on {}: embedded as PNG",
                    png.display()
                );
                return png.to_path_buf();
            }
        }
        match std::fs::metadata(&webp) {
            Ok(m) if (m.len() as usize) < bytes.len() => webp,
            _ => png.to_path_buf(),
        }
    }
}

fn sorted_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let manual = manifest
        .join("../../docs/manual")
        .canonicalize()
        .expect("docs/manual exists");
    println!("cargo:rerun-if-changed={}", manual.display());
    let mut out = String::new();
    let lit = |p: &Path| format!("{:?}", p.display().to_string());
    writeln!(
        out,
        "pub static INDEX_JSON: &str = include_str!({});",
        lit(&manual.join("index.json"))
    )
    .unwrap();
    out.push_str("pub static CHAPTER_FILES: &[(&str, &str, &str)] = &[\n");
    let mut langs: Vec<PathBuf> = std::fs::read_dir(&manual)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n != "images"))
        .collect();
    langs.sort();
    for dir in &langs {
        println!("cargo:rerun-if-changed={}", dir.display());
        let lang = dir.file_name().unwrap().to_string_lossy().into_owned();
        for f in sorted_files(dir, "md") {
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            writeln!(out, "    ({lang:?}, {name:?}, include_str!({})),", lit(&f)).unwrap();
        }
    }
    out.push_str("];\n");
    // Developer docs the chapters link to (`../../plugins.md`): shown in
    // the help window too, so those links work in the app.
    let mut dev: Vec<String> = Vec::new();
    for dir in &langs {
        for f in sorted_files(dir, "md") {
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            for part in text.split("](../../").skip(1) {
                let name = part.split([')', '#']).next().unwrap_or("");
                if name.ends_with(".md") && !name.contains('/') && !dev.iter().any(|d| d == name) {
                    dev.push(name.to_string());
                }
            }
        }
    }
    // The licence notices (Help → Third-Party Licences, Settings).
    if !dev.iter().any(|d| d == "third-party-licenses.md") {
        dev.push("third-party-licenses.md".to_string());
    }
    dev.sort();
    out.push_str("pub static DEV_DOCS: &[(&str, &str)] = &[\n");
    for name in dev {
        let f = manual.join("..").join(&name);
        if f.is_file() {
            println!("cargo:rerun-if-changed={}", f.display());
            writeln!(
                out,
                "    ({name:?}, include_str!({})),",
                lit(&f.canonicalize().unwrap())
            )
            .unwrap();
        }
    }
    out.push_str("];\n");
    out.push_str("pub static IMAGE_FILES: &[(&str, &[u8])] = &[\n");
    let images = manual.join("images");
    println!("cargo:rerun-if-changed={}", images.display());
    let (mut keys, mut pngs) = (Vec::new(), Vec::new());
    if let Ok(rd) = std::fs::read_dir(&images) {
        let mut dirs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for dir in dirs {
            println!("cargo:rerun-if-changed={}", dir.display());
            let lang = dir.file_name().unwrap().to_string_lossy().into_owned();
            for f in sorted_files(&dir, "png") {
                let name = f.file_name().unwrap().to_string_lossy().into_owned();
                keys.push(format!("{lang}/{name}"));
                pngs.push(f);
            }
        }
    }
    let cache = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("manual-webp");
    for (key, file) in keys.iter().zip(webp::compress_all(&pngs, &cache)) {
        writeln!(out, "    ({key:?}, include_bytes!({})),", lit(&file)).unwrap();
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("manual_files.rs");
    std::fs::write(dest, out).unwrap();
}
