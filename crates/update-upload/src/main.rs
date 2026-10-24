//! `manta-update-upload`: upload release files over FTPS (explicit TLS) with
//! Manta's own FTP backend. Called by `scripts/publish-update.sh`.
//!
//! ```text
//! printf '%s\n' "$pw" | manta-update-upload [--user USER] [--data-channel auto|require|off] \
//!     ftp://host[:port]/dir/ Manta-1.2.3.zip update.json.sig update.json
//! manta-update-upload [OPTIONS] --list ftp://host/dir/       # list a remote folder (read only)
//! manta-update-upload [OPTIONS] --mkdir ftp://host/dir/      # create a folder and its parents
//! manta-update-upload [OPTIONS] --tree LOCAL_DIR [--exclude PAT]... [--dry-run] ftp://host/dir/
//!     # upload LOCAL_DIR's contents recursively (deletes nothing on the server)
//! ```
//!
//! `--exclude PAT`: a name (matched at any depth, e.g. `.DS_Store`) or a
//! relative path with a `/` (that file or folder). `--dry-run` (with `--tree`)
//! prints what would be uploaded without connecting or reading a password.
//!
//! - The password is read from the first line of stdin, never from the
//!   command line (it would show up in `ps`), and never printed.
//! - The user: `--user`, the `user@` part of the URL, `$MANTA_FTP_USER`, or
//!   the account of the Keychain item `$MANTA_FTP_KEYCHAIN_SERVICE`
//!   (default `manta-update-ftp`; attributes only, no password is read).
//! - Data channel (`--data-channel`, or `$MANTA_FTP_DATA_TLS`): `auto`
//!   (default: encrypted, clear data connections if the network resets
//!   encrypted ones; the login stays encrypted), `require`, `off`.

use std::io::BufRead;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use manta_core::{Auth, ConnectionConfig, ConnectionNotice, FtpDataChannel, FtpOptions, Protocol};
use manta_update_upload::{
    collect_tree, ensure_dir, list_lines, upload_all, upload_tree, Destination,
};

const USAGE: &str = "usage: manta-update-upload [--user USER] [--data-channel auto|require|off] \
                     ftp://host[:port]/dir/ FILE...\n   \
                     or: manta-update-upload [OPTIONS] --list ftp://host/dir/\n   \
                     or: manta-update-upload [OPTIONS] --mkdir ftp://host/dir/\n   \
                     or: manta-update-upload [OPTIONS] --tree LOCAL_DIR [--exclude PAT]... [--dry-run] ftp://host/dir/\n\
                     (password on stdin)";

enum Mode {
    Files,
    List,
    Mkdir,
    Tree(PathBuf),
}

fn data_channel(s: &str) -> Result<FtpDataChannel, String> {
    match s {
        "auto" => Ok(FtpDataChannel::Auto),
        "require" => Ok(FtpDataChannel::Require),
        "off" => Ok(FtpDataChannel::Off),
        other => Err(format!(
            "unknown data channel \"{other}\" (auto, require or off)"
        )),
    }
}

/// The account of a Keychain generic password item (attributes only).
fn keychain_account(service: &str) -> Option<String> {
    let out = Command::new("security")
        .args(["find-generic-password", "-s", service])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        l.trim()
            .strip_prefix("\"acct\"<blob>=\"")?
            .strip_suffix('"')
            .map(str::to_string)
    })
}

fn run() -> Result<(), String> {
    let mut user = None;
    let mut policy = match std::env::var("MANTA_FTP_DATA_TLS") {
        Ok(v) if !v.is_empty() => data_channel(&v)?,
        _ => FtpDataChannel::Auto,
    };
    let mut positional = Vec::new();
    let mut mode = Mode::Files;
    let mut excludes = Vec::new();
    let mut dry_run = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--user" => user = Some(args.next().ok_or("--user needs a value")?),
            "--list" => mode = Mode::List,
            "--mkdir" => mode = Mode::Mkdir,
            "--tree" => {
                mode = Mode::Tree(PathBuf::from(
                    args.next().ok_or("--tree needs a local folder")?,
                ))
            }
            "--exclude" => excludes.push(args.next().ok_or("--exclude needs a value")?),
            "--dry-run" => dry_run = true,
            "--data-channel" => {
                policy = data_channel(&args.next().ok_or("--data-channel needs a value")?)?
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}\n{USAGE}")),
            _ => positional.push(a),
        }
    }
    let enough = match mode {
        Mode::Files => positional.len() >= 2,
        _ => positional.len() == 1,
    };
    if !enough {
        return Err(USAGE.to_string());
    }
    if dry_run && !matches!(mode, Mode::Tree(_)) {
        return Err("--dry-run only works with --tree".into());
    }
    if !excludes.is_empty() && !matches!(mode, Mode::Tree(_)) {
        return Err("--exclude only works with --tree".into());
    }
    let dest = Destination::parse(&positional[0])?;
    let files: Vec<PathBuf> = positional[1..].iter().map(PathBuf::from).collect();
    // Check local input before asking for anything.
    let tree = match &mode {
        Mode::Tree(dir) => Some(collect_tree(dir, &excludes)?),
        _ => None,
    };
    if let (Some(tree), true) = (&tree, dry_run) {
        let total: u64 = tree.iter().map(|f| f.size).sum();
        println!(
            "  dry run: would upload {} file(s), {total} bytes, to ftp://{}{} (in this order):",
            tree.len(),
            dest.host,
            dest.dir
        );
        for f in tree {
            println!("  {:>10}  {}", f.size, dest.dir.join(&f.rel));
        }
        return Ok(());
    }

    let service =
        std::env::var("MANTA_FTP_KEYCHAIN_SERVICE").unwrap_or_else(|_| "manta-update-ftp".into());
    let user = user
        .or(dest.user.clone())
        .or_else(|| {
            std::env::var("MANTA_FTP_USER")
                .ok()
                .filter(|u| !u.is_empty())
        })
        .or_else(|| keychain_account(&service))
        .ok_or_else(|| {
            format!("no FTP user: pass --user or create the Keychain item \"{service}\"")
        })?;

    let mut password = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut password)
        .map_err(|e| format!("reading the password from stdin: {e}"))?;
    let password = password.trim_end_matches(['\r', '\n']).to_string();
    if password.is_empty() {
        return Err("no password on stdin".into());
    }

    let cfg = ConnectionConfig {
        name: "manta-update-upload".into(),
        protocol: Protocol::Ftps,
        host: dest.host.clone(),
        port: dest.port,
        username: user.clone(),
        auth: Auth::Password(password),
        initial_path: None,
        s3: None,
        container: None,
        ftp: Some(Box::new(FtpOptions {
            data_channel: policy,
        })),
        webdav: None,
        smb: None,
    };
    println!(
        "  connecting to {}:{} as {user} (FTPS, data channel {policy:?})",
        dest.host, dest.port
    );
    let fs = manta_vfs::connect(&cfg).map_err(|e| format!("cannot connect: {e}"))?;
    drop(cfg);
    let mut noticed = false;
    let mut say = |line: String| {
        if !noticed && fs.notice() == Some(ConnectionNotice::ClearDataChannel) {
            noticed = true;
            println!(
                "  note: this network resets encrypted FTP data connections; continuing with clear \
                 data connections (the login stays encrypted, the files are signed)"
            );
        }
        println!("  {line}");
    };
    match mode {
        Mode::Files => upload_all(&*fs, &dest.dir, &files, &mut say),
        Mode::List => {
            println!("  {}:", dest.dir);
            for line in list_lines(&*fs, &dest.dir)? {
                println!("  {line}");
            }
            Ok(())
        }
        Mode::Mkdir => {
            ensure_dir(&*fs, &dest.dir)?;
            println!("  {} exists", dest.dir);
            Ok(())
        }
        Mode::Tree(_) => upload_tree(
            &*fs,
            &dest.dir,
            tree.as_deref().unwrap_or_default(),
            &mut say,
        ),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("manta-update-upload: {e}");
            ExitCode::FAILURE
        }
    }
}
