//! iCloud Drive's top folder lists the apps' folders like Finder, and they
//! can't be renamed or deleted. A fake home (`MANTA_CLOUD_HOME`): the only
//! test in this binary, so setting the variable races with nothing.

use manta_core::{Vfs, VfsError, VfsPath};
use manta_vfs::cloud_local::HOME_OVERRIDE_ENV;
use manta_vfs::LocalFs;

fn vp(p: &std::path::Path) -> VfsPath {
    VfsPath::new(&p.to_string_lossy())
}

#[test]
fn icloud_top_folder_lists_app_folders_and_protects_them() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let mobile = home.join("Library/Mobile Documents");
    let top = mobile.join("com~apple~CloudDocs");
    std::fs::create_dir_all(top.join("Notes")).unwrap();
    let obsidian = mobile.join("iCloud~md~obsidian/Documents");
    std::fs::create_dir_all(obsidian.join("Vault")).unwrap();
    // No documents: not shown (like Finder).
    std::fs::create_dir_all(mobile.join("iCloud~com~empty/Documents")).unwrap();
    std::env::set_var(HOME_OVERRIDE_ENV, &home);

    let fs = LocalFs::new();
    let mut entries = fs.list(&vp(&top)).unwrap();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let names: Vec<(&str, String)> = entries
        .iter()
        .map(|e| (e.name.as_str(), e.path.to_string()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Notes", top.join("Notes").to_string_lossy().into_owned()),
            ("obsidian", obsidian.to_string_lossy().into_owned()),
        ]
    );
    assert!(entries[1].is_dir());
    // Other folders list as before.
    assert_eq!(fs.list(&vp(&top.join("Notes"))).unwrap().len(), 0);

    // The app's folder stays; what is inside it can change.
    let refused = |r: Result<(), VfsError>| matches!(r, Err(VfsError::PermissionDenied(_)));
    assert!(refused(fs.rename(&vp(&obsidian), &vp(&top.join("Moved")))));
    assert!(refused(fs.remove_dir(&vp(&obsidian))));
    assert!(obsidian.is_dir());
    fs.rename(&vp(&obsidian.join("Vault")), &vp(&obsidian.join("Notes")))
        .unwrap();
    fs.remove_dir(&vp(&obsidian.join("Notes"))).unwrap();

    std::env::remove_var(HOME_OVERRIDE_ENV);
}
