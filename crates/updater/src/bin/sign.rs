//! Signing tool used by `scripts/publish-update.sh`.
//!
//! ```text
//! manta-update-sign manifest --key KEY --zip ZIP --version V --notes-en FILE [--notes-de FILE]
//!     [--channel stable|beta] [--min-macos 11.0] [--url URL] [--app-id com.manta-filemanager.manta]
//!     [--existing update.json [--existing-sig update.json.sig]] [--keep 10] --out DIR
//!   Adds/replaces the release for V (size, SHA-256, Ed25519 signature) in the
//!   existing manifest (whose signature must verify with the same key), keeps the
//!   newest N releases and writes DIR/update.json and DIR/update.json.sig.
//!
//! manta-update-sign verify (--pubkey FILE | --key KEY) --manifest update.json [--sig FILE] [--zip ZIP]
//!   Checks the manifest signature, every release signature and, with --zip,
//!   that the ZIP matches its release.
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::Utc;
use manta_updater::download::sha256_file;
use manta_updater::{keys, Channel, Manifest, Notes, Release, Version, DEFAULT_APP_ID, SCHEMA};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("manifest") => parse(&args[1..]).and_then(|o| cmd_manifest(&o)),
        Some("verify") => parse(&args[1..]).and_then(|o| cmd_verify(&o)),
        _ => Err("usage: manta-update-sign manifest|verify [options] (see the source header or docs/updates.md)".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type Opts = HashMap<String, String>;

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = HashMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let k = a
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected argument {a}"))?;
        let v = it.next().ok_or_else(|| format!("--{k} needs a value"))?;
        o.insert(k.to_string(), v.clone());
    }
    Ok(o)
}

fn req<'a>(o: &'a Opts, k: &str) -> Result<&'a str, String> {
    o.get(k)
        .map(String::as_str)
        .ok_or_else(|| format!("--{k} is required"))
}

fn read(p: &str) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))
}

fn cmd_manifest(o: &Opts) -> Result<(), String> {
    let key = keys::parse_private_key(&read(req(o, "key")?)?).map_err(|e| e.to_string())?;
    let vk = key.verifying_key();
    let app_id = o
        .get("app-id")
        .map(String::as_str)
        .unwrap_or(DEFAULT_APP_ID);
    let zip = PathBuf::from(req(o, "zip")?);
    let version = Version::parse(req(o, "version")?).map_err(|e| format!("--version: {e}"))?;
    let channel = match o.get("channel") {
        Some(c) => Some(Channel::parse(c).ok_or("--channel must be stable or beta")?),
        None => None,
    };
    let keep: usize = o
        .get("keep")
        .map(|k| k.parse().map_err(|_| "--keep must be a number"))
        .transpose()?
        .unwrap_or(10);
    let out = PathBuf::from(req(o, "out")?);
    let notes = Notes {
        en: read(req(o, "notes-en")?)?.trim().to_string(),
        de: o
            .get("notes-de")
            .map(|p| read(p))
            .transpose()?
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    };
    let (sha256, size) = sha256_file(&zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let url = o.get("url").cloned().unwrap_or_else(|| {
        zip.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    });

    let mut release = Release {
        version: version.clone(),
        channel,
        published: Utc::now().fixed_offset(),
        min_macos: o.get("min-macos").cloned().unwrap_or_else(|| "11.0".into()),
        notes,
        url,
        size,
        sha256,
        signature: String::new(),
    };
    keys::sign_release(&key, app_id, &mut release).map_err(|e| e.to_string())?;

    let mut manifest = match o.get("existing") {
        Some(p) if Path::new(p).is_file() => {
            let bytes = std::fs::read(p).map_err(|e| format!("{p}: {e}"))?;
            let sig_path = o
                .get("existing-sig")
                .cloned()
                .unwrap_or_else(|| format!("{p}.sig"));
            let sig = read(&sig_path).map_err(|e| {
                format!(
                    "the existing manifest has no signature ({e}); remove it or start a new feed"
                )
            })?;
            keys::verify_manifest(&vk, &bytes, &sig).map_err(|e| {
                format!("the existing update.json does not verify with your key ({e}). Refusing to re-sign entries someone else may have written.")
            })?;
            let m = Manifest::parse(&bytes).map_err(|e| e.to_string())?;
            for r in &m.releases {
                keys::verify_release(&vk, app_id, r)
                    .map_err(|e| format!("existing entry {}: {e}", r.version))?;
            }
            if m.app_id != app_id {
                return Err(format!(
                    "the existing feed is for {}, not {app_id}",
                    m.app_id
                ));
            }
            m
        }
        _ => Manifest {
            schema: SCHEMA,
            app_id: app_id.into(),
            channel: Channel::Stable,
            generated: None,
            releases: vec![],
        },
    };
    manifest.releases.retain(|r| r.version != version);
    manifest.releases.push(release);
    manifest.releases.sort_by(|a, b| b.version.cmp(&a.version));
    manifest.releases.truncate(keep.max(1));
    manifest.generated = Some(Utc::now().fixed_offset());
    manifest.validate().map_err(|e| e.to_string())?;

    let json = manifest.to_json();
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(out.join("update.json"), &json).map_err(|e| e.to_string())?;
    std::fs::write(
        out.join("update.json.sig"),
        keys::sign_manifest(&key, json.as_bytes()),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "update.json: {} release(s): {}",
        manifest.releases.len(),
        manifest
            .releases
            .iter()
            .map(|r| format!("{} ({})", r.version, r.effective_channel().as_str()))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

fn cmd_verify(o: &Opts) -> Result<(), String> {
    let vk = match (o.get("pubkey"), o.get("key")) {
        (Some(p), _) => keys::parse_public_key(&read(p)?).map_err(|e| e.to_string())?,
        (None, Some(k)) => keys::parse_private_key(&read(k)?)
            .map_err(|e| e.to_string())?
            .verifying_key(),
        _ => return Err("--pubkey or --key is required".into()),
    };
    let mpath = req(o, "manifest")?;
    let bytes = std::fs::read(mpath).map_err(|e| format!("{mpath}: {e}"))?;
    let sig = read(
        &o.get("sig")
            .cloned()
            .unwrap_or_else(|| format!("{mpath}.sig")),
    )?;
    keys::verify_manifest(&vk, &bytes, &sig).map_err(|e| e.to_string())?;
    let m = Manifest::parse(&bytes).map_err(|e| e.to_string())?;
    for r in &m.releases {
        keys::verify_release(&vk, &m.app_id, r).map_err(|e| e.to_string())?;
    }
    if let Some(z) = o.get("zip") {
        let (sha, size) = sha256_file(Path::new(z)).map_err(|e| format!("{z}: {e}"))?;
        let r = m
            .releases
            .iter()
            .find(|r| r.sha256 == sha && r.size == size)
            .ok_or(format!("{z} matches no release"))?;
        println!("{z}: matches release {}", r.version);
    }
    println!(
        "OK: manifest and {} release signature(s) verified",
        m.releases.len()
    );
    Ok(())
}
