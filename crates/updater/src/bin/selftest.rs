//! End-to-end exercise of the update engine on a real bundle, without the UI.
//!
//! ```text
//! manta-update-selftest --feed URL --app /path/to/Manta.app
//!     [--channel stable|beta] [--pubkey FILE] [--download-dir DIR]
//!     [--backup trash|delete|keep] [--relaunch]
//! ```
//! Reads the app's bundle ID and version, then check → download → prepare →
//! install into the given bundle path. `--relaunch` starts the relaunch helper
//! for this process, which opens the new app once the selftest has exited.
//! Exit codes: 0 updated, 3 already up to date, 1 error.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use manta_updater::bundle::read_bundle_info;
use manta_updater::{
    check_with, download_with, install, keys, prepare_with, relaunch_after_exit, BackupDisposal,
    Channel, CheckOptions, PrepareOptions, Version,
};

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(3),
        Err(e) => {
            eprintln!("selftest: error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, String> {
    let mut o: HashMap<String, String> = HashMap::new();
    let mut flags = Vec::new();
    let mut args = std::env::args().skip(1).peekable();
    while let Some(a) = args.next() {
        let k = a
            .strip_prefix("--")
            .ok_or(format!("unexpected argument {a}"))?
            .to_string();
        if k == "relaunch" {
            flags.push(k);
        } else {
            o.insert(
                k.clone(),
                args.next().ok_or(format!("--{k} needs a value"))?,
            );
        }
    }
    let feed = o.get("feed").ok_or("--feed is required")?;
    let app = PathBuf::from(o.get("app").ok_or("--app is required")?);
    let channel = Channel::parse(o.get("channel").map(String::as_str).unwrap_or("stable"))
        .ok_or("bad --channel")?;
    let key = match o.get("pubkey") {
        Some(p) => {
            keys::parse_public_key(&std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?)
        }
        None => keys::embedded_public_key(),
    }
    .map_err(|e| e.to_string())?;
    let backup = match o.get("backup").map(String::as_str).unwrap_or("delete") {
        "trash" => BackupDisposal::Trash,
        "keep" => BackupDisposal::Keep,
        "delete" => BackupDisposal::Delete,
        b => return Err(format!("bad --backup {b}")),
    };
    let dl_dir = o
        .get("download-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("manta-selftest"));

    let info = read_bundle_info(&app).map_err(|e| e.to_string())?;
    let current = Version::parse(&info.version)
        .map_err(|e| format!("bundle version {}: {e}", info.version))?;
    println!("app: {} {} ({})", app.display(), current, info.bundle_id);

    let opts = CheckOptions {
        key,
        app_id: info.bundle_id.clone(),
        os_version: manta_updater::check::macos_version(),
    };
    let Some(release) =
        check_with(&opts, feed, &current, channel, None).map_err(|e| e.to_string())?
    else {
        println!("up to date");
        return Ok(false);
    };
    println!(
        "available: {} ({} bytes) from {}",
        release.version, release.size, release.url
    );

    let cancel = AtomicBool::new(false);
    let mut last_pct = u64::MAX;
    let mut progress = |done: u64, total: u64| {
        let pct = done * 100 / total.max(1);
        if pct / 10 != last_pct / 10 || done == total {
            println!("download: {pct}% ({done}/{total})");
            last_pct = pct;
        }
    };
    let dl = download_with(
        &opts.key,
        &opts.app_id,
        &release,
        &dl_dir,
        &mut progress,
        &cancel,
    )
    .map_err(|e| e.to_string())?;
    println!("verified: {}", dl.path().display());

    let prepared = prepare_with(
        &dl,
        &PrepareOptions {
            expected_bundle_id: info.bundle_id.clone(),
            current_bundle: Some(app.clone()),
            work_dir: None,
        },
    )
    .map_err(|e| e.to_string())?;
    println!(
        "prepared: {} (signing: {:?})",
        prepared.app_path.display(),
        prepared.signing
    );

    let mut plan = install(prepared, &app).map_err(|e| e.to_string())?;
    plan.backup = backup;
    println!(
        "install plan: target {} needs_admin={}",
        plan.target.display(),
        plan.needs_admin
    );
    let outcome = plan.apply().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(dl.path());
    let now = read_bundle_info(&outcome.installed).map_err(|e| e.to_string())?;
    println!(
        "installed: {} is now {}; backup: {:?}",
        outcome.installed.display(),
        now.version,
        outcome.backup
    );
    if flags.iter().any(|f| f == "relaunch") {
        relaunch_after_exit(std::process::id(), &outcome.installed).map_err(|e| e.to_string())?;
        println!("relaunch helper started; exiting");
    }
    Ok(true)
}
