//! Embeds the user manual (`docs/manual`) into the binary: every chapter of
//! every language, `index.json` and the screenshots in `images/<lang>/`.
//! Generates `$OUT_DIR/manual_files.rs` (used by `src/help/manual.rs`), so
//! the release binary is self-contained and new chapters or images are
//! picked up without code changes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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
                writeln!(out, "    (\"{lang}/{name}\", include_bytes!({})),", lit(&f)).unwrap();
            }
        }
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("manual_files.rs");
    std::fs::write(dest, out).unwrap();
}
