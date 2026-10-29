//! Ejectable volumes in the sidebar (like Finder): disk images, USB sticks
//! and external drives are listed under "Ejectable", each with an eject
//! button that is always shown, and a context menu like Finder's.

use super::*;
use crate::app::Action;
use egui_kittest::kittest::Queryable;

/// The fixture's places plus an opened disk image, "Installer" (a folder
/// of the fixture).
fn with_disk_image(fx: &Fixture) -> H {
    let root = fx.root.clone();
    std::fs::create_dir_all(root.join("Installer")).unwrap();
    harness_init(
        fx,
        WIDE,
        Theme::Light,
        false,
        Box::new(|ctx: &egui::Context| PluginHost::new(ctx.clone(), Vec::new(), None)),
        move |init| {
            let (locations, mut devices) = places(&root);
            devices.push(Place {
                icon: icons::HARD_DRIVE,
                name: "Installer".into(),
                path: root.join("Installer"),
                network: false,
                ejectable: true,
                image: None,
            });
            init.places = Some((locations, devices));
        },
    )
}

fn act(h: &mut H, a: Action) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, a);
    h.run_steps(3);
    settle(h);
}

fn path(h: &mut H) -> VfsPath {
    let side = app(h).active;
    app(h).pane(side).tab().loc.path.clone()
}

#[test]
fn ejectable_volumes_have_their_own_section_and_an_eject_button() {
    let fx = fixture();
    let mut h = with_disk_image(&fx);
    // "Ejectable" with the disk image; "Macintosh HD" and the backup drive
    // stay under "Devices" without an eject button.
    assert!(h.query_by_label(t!("sidebar.ejectable")).is_some());
    assert_eq!(h.query_all_by_label(t!("sidebar.eject")).count(), 1);

    // Nothing ejectable mounted: no section.
    app(&mut h).devices.retain(|d| !d.ejectable);
    h.run_steps(2);
    assert!(h.query_by_label(t!("sidebar.ejectable")).is_none());
    assert_eq!(h.query_all_by_label(t!("sidebar.eject")).count(), 0);
}

#[test]
fn volume_menu_actions() {
    let fx = fixture();
    let mut h = with_disk_image(&fx);
    let volume = fx.root.join("Installer");
    let side = app(&mut h).active;

    // Open in New Tab: one more tab, showing the volume.
    let tabs = app(&mut h).pane(side).tabs.len();
    act(&mut h, Action::OpenLocalInNewTab(volume.clone()));
    assert_eq!(app(&mut h).pane(side).tabs.len(), tabs + 1);
    assert_eq!(path(&mut h), VfsPath::new(&volume.to_string_lossy()));

    // Show in Enclosing Folder: not a disk image, so the volume is selected
    // in its folder.
    act(&mut h, Action::RevealVolume(volume.clone()));
    let start = Instant::now();
    while path(&mut h) != VfsPath::new(&fx.root.to_string_lossy()) {
        assert!(start.elapsed() < Duration::from_secs(10), "not revealed");
        h.run_steps(2);
        settle(&mut h);
    }
    let selected: Vec<String> = app(&mut h)
        .pane(side)
        .tab()
        .selected_entries()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(selected, ["Installer"]);
}
