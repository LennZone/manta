//! `cargo run -p manta-updater --bin manta-update-keygen [-- options]`
//!
//! Creates the Ed25519 key pair that signs Manta updates.
//!
//!   --private-key PATH   default ~/.config/manta/update-signing-key (mode 0600)
//!   --public-key PATH    default <repo>/packaging/update-public-key.txt
//!   --force              replace an existing private key (old updates can then
//!                        no longer be verified by apps that embed the old key!)
//!   --show-public        print the public key of the existing private key and
//!                        (re)write the public key file; generates nothing

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::process::ExitCode;

use manta_updater::keys;

fn default_private() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/manta/update-signing-key")
}

fn default_public() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging/update-public-key.txt")
}

fn main() -> ExitCode {
    let mut private = default_private();
    let mut public = default_public();
    let mut force = false;
    let mut show = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--private-key" => private = args.next().map(PathBuf::from).unwrap_or(private),
            "--public-key" => public = args.next().map(PathBuf::from).unwrap_or(public),
            "--force" => force = true,
            "--show-public" => show = true,
            "-h" | "--help" => {
                println!("usage: manta-update-keygen [--private-key PATH] [--public-key PATH] [--force] [--show-public]");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("error: unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    match run(&private, &public, force, show) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(private: &PathBuf, public: &PathBuf, force: bool, show: bool) -> Result<(), String> {
    let key = if show {
        let text =
            std::fs::read_to_string(private).map_err(|e| format!("{}: {e}", private.display()))?;
        keys::parse_private_key(&text).map_err(|e| e.to_string())?
    } else {
        if private.exists() && !force {
            return Err(format!(
                "{} already exists. Refusing to overwrite it: apps built with its public key would no longer accept updates.\n\
                 Use --show-public to rewrite the public key file from it, or --force to really replace it.",
                private.display()
            ));
        }
        let key = keys::generate_private_key().map_err(|e| e.to_string())?;
        if let Some(dir) = private.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        let tmp = private.with_extension("tmp");
        let _ = std::fs::remove_file(&tmp);
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.write_all(keys::private_key_file_contents(&key).as_bytes())
            .map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, private).map_err(|e| format!("{}: {e}", private.display()))?;
        std::fs::set_permissions(private, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        key
    };
    let vk = key.verifying_key();
    if let Some(dir) = public.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(public, keys::public_key_file_contents(&vk))
        .map_err(|e| format!("{}: {e}", public.display()))?;

    println!("Public key:  {}", keys::public_key_b64(&vk));
    println!("  written to {}", public.display());
    if !show {
        println!("Private key: {} (mode 0600)", private.display());
        println!();
        println!("IMPORTANT");
        println!("  * Back up the private key NOW (password manager or an encrypted USB stick).");
        println!("    If you lose it, installed copies of Manta can never be updated again;");
        println!("    users would have to download a new version by hand.");
        println!("  * Never commit, upload or share it. It is outside the repository on purpose,");
        println!("    and .gitignore blocks *signing-key* files as a second guard.");
        println!("  * Commit packaging/update-public-key.txt and rebuild: the key is compiled in.");
    }
    Ok(())
}
