//! Dragging folders onto the sidebar's Favorites: local folders, a mix of
//! folders and files, folders on a server, re-sorting, and duplicates.
//!
//! Behaviour tests step the real app without a GPU; `render_favorites_drop`
//! needs one and writes PNGs to `$MANTA_RENDER_DIR`.

use super::*;
use crate::pane::DragPayload;
use crate::sidebar::FavoriteDrag;
use egui::Modifiers;

fn behaviour(fx: &Fixture) -> H {
    harness_with(fx, WIDE, Theme::Light, false)
}

fn local_path(fx: &Fixture, name: &str) -> VfsPath {
    VfsPath::new(&fx.root.join(name).to_string_lossy())
}

fn fav_names(h: &mut H) -> Vec<String> {
    app(h)
        .favorites
        .items
        .iter()
        .map(|f| f.display_name())
        .collect()
}

/// A drag from pane 0 with `items` ((path, is folder)).
fn payload(h: &mut H, items: &[(VfsPath, bool)], conn: Option<u64>) -> DragPayload {
    let fs = app(h).pane(Side::LEFT).tab().loc.fs.clone();
    DragPayload {
        from: Side::LEFT,
        fs,
        paths: items.iter().map(|(p, _)| p.clone()).collect(),
        dirs: items.iter().map(|(_, d)| *d).collect(),
        conn,
        archive: false,
    }
}

/// Hover `pos` with a drag in flight, then release there. `make` builds
/// the payload (set again before the release, as egui clears it on press).
fn drop_at<P: std::any::Any + Send + Sync>(h: &mut H, pos: egui::Pos2, make: impl Fn(&mut H) -> P) {
    let ctx = h.ctx.clone();
    let p = make(h);
    egui::DragAndDrop::set_payload(&ctx, p);
    // Press on the (not draggable) section header, not on a row: a press
    // held on a favorite row long enough starts egui's own drag of that
    // row, which would replace the payload (flaky under load).
    let first = app(h).fav_rects[0];
    let press = egui::pos2(first.center().x, first.top() - 16.0);
    h.hover_at(press);
    h.event(egui::Event::PointerButton {
        pos: press,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    let p = make(h);
    egui::DragAndDrop::set_payload(&ctx, p);
    h.hover_at(pos);
    h.step();
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
}

/// Just below the middle of favorite row `i` (lands after it) or above it.
fn between(h: &mut H, i: usize, after: bool) -> egui::Pos2 {
    let r = app(h).fav_rects[i];
    egui::pos2(
        r.center().x,
        if after {
            r.bottom() - 3.0
        } else {
            r.top() + 3.0
        },
    )
}

#[test]
fn dropping_a_local_folder_adds_it_where_the_line_is() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    assert_eq!(
        fav_names(&mut h),
        vec!["Projects", "Production web", "anna@files.example.org"]
    );
    let src = local_path(&fx, "src");
    // Between "Projects" and "Production web".
    let pos = between(&mut h, 0, true);
    drop_at(&mut h, pos, |h| payload(h, &[(src.clone(), true)], None));
    assert_eq!(
        fav_names(&mut h),
        vec![
            "Projects",
            "src",
            "Production web",
            "anna@files.example.org"
        ]
    );
    let Favorite::Local { path, .. } = &app(&mut h).favorites.items[1] else {
        panic!("a local favorite")
    };
    assert_eq!(path, &src.to_string());
    assert!(
        app(&mut h)
            .toasts
            .titles()
            .iter()
            .any(|t| t.contains("src")),
        "a confirmation"
    );
}

#[test]
fn folders_and_files_only_the_folders_are_added() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let items = [
        (local_path(&fx, "photos"), true),
        (local_path(&fx, "notes.txt"), false),
        (local_path(&fx, "Design"), true),
    ];
    let pos = between(&mut h, 2, true);
    drop_at(&mut h, pos, |h| payload(h, &items, None));
    assert_eq!(
        fav_names(&mut h),
        vec![
            "Projects",
            "Production web",
            "anna@files.example.org",
            "photos",
            "Design"
        ],
        "folders in drag order at the end; the file is left out"
    );

    // Only files: nothing is added (the cursor says no while hovering).
    let pos = between(&mut h, 0, false);
    let files = [(local_path(&fx, "notes.txt"), false)];
    let ctx = h.ctx.clone();
    let p = payload(&mut h, &files, None);
    egui::DragAndDrop::set_payload(&ctx, p);
    h.hover_at(pos);
    h.step();
    assert_eq!(
        crate::sidebar::favorites_drop_hover(&ctx),
        Some(0),
        "the drag badge says folders only"
    );
    assert_eq!(
        h.output().platform_output.cursor_icon,
        egui::CursorIcon::NotAllowed
    );
    drop_at(&mut h, pos, |h| payload(h, &files, None));
    assert_eq!(fav_names(&mut h).len(), 5);
}

#[test]
fn a_folder_on_a_server_becomes_a_favorite_of_that_connection() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let Favorite::Connection { config, .. } = app(&mut h).favorites.items[1].clone() else {
        unreachable!()
    };
    let mut config = config;
    config.auth = manta_core::Auth::Password("secret".into());
    let fs: manta_core::SharedVfs = std::sync::Arc::new(manta_vfs::LocalFs::new());
    app(&mut h).connections.push(crate::app::Connection {
        id: 42,
        label: "deploy@web-01".into(),
        config: config.clone(),
        fs,
        notice_shown: true,
    });
    let site = VfsPath::new("/var/www/site");
    let pos = between(&mut h, 0, false);
    drop_at(&mut h, pos, |h| {
        payload(h, &[(site.clone(), true)], Some(42))
    });
    let Favorite::Connection { config: fav, .. } = app(&mut h).favorites.items[0].clone() else {
        panic!("a connection favorite first: {:?}", fav_names(&mut h))
    };
    assert_eq!(
        fav.initial_path.as_deref(),
        Some("/var/www/site"),
        "starts in the folder"
    );
    assert_eq!(
        (fav.host.as_str(), fav.port, fav.username.as_str()),
        ("web-01.example.com", 22, "deploy")
    );
    assert_eq!(fav.protocol, config.protocol);
    assert_eq!(fav.name, "Production web – site");
    assert_eq!(
        fav.auth,
        manta_core::Auth::Password(String::new()),
        "no secret is saved"
    );
}

#[test]
fn favorites_re_sort_by_dragging_and_the_order_is_saved() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let file = fx.root.join("favorites-test.json");
    app(&mut h).favorites_path = Some(file.clone());
    // Drag "Projects" (0) below the last one.
    let pos = between(&mut h, 2, true);
    drop_at(&mut h, pos, |_| FavoriteDrag(0));
    assert_eq!(
        fav_names(&mut h),
        vec!["Production web", "anna@files.example.org", "Projects"]
    );
    let saved = Favorites::load(&file).unwrap();
    assert_eq!(
        saved
            .items
            .iter()
            .map(|f| f.display_name())
            .collect::<Vec<_>>(),
        fav_names(&mut h),
        "persisted"
    );
    // And back to the top.
    let pos = between(&mut h, 0, false);
    drop_at(&mut h, pos, |_| FavoriteDrag(2));
    assert_eq!(
        fav_names(&mut h),
        vec!["Projects", "Production web", "anna@files.example.org"]
    );
    // Dropping a favorite onto its own place changes nothing.
    let pos = between(&mut h, 1, false);
    drop_at(&mut h, pos, |_| FavoriteDrag(1));
    assert_eq!(
        fav_names(&mut h),
        vec!["Projects", "Production web", "anna@files.example.org"]
    );
}

#[test]
fn a_duplicate_is_not_added_and_the_existing_one_flashes() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let projects = VfsPath::new(&format!("{}/", fx.root.to_string_lossy()));
    let pos = between(&mut h, 2, true);
    drop_at(&mut h, pos, |h| {
        payload(h, &[(projects.clone(), true)], None)
    });
    assert_eq!(fav_names(&mut h).len(), 3, "no second “Projects”");
    let flash = app(&mut h).fav_flash.map(|(i, _)| i);
    assert_eq!(flash, Some(0), "the existing favorite flashes");
    assert!(app(&mut h)
        .toasts
        .titles()
        .iter()
        .any(|t| t.contains("Projects")));
    // The flash ends by itself.
    let now = h.ctx.input(|i| i.time);
    app(&mut h).fav_flash = Some((0, Some(now - 2.0)));
    h.run_steps(2);
    assert!(app(&mut h).fav_flash.is_none());
}

#[test]
fn render_favorites_drop() {
    require_gpu!();
    let fx = fixture();
    let dir = render_dir("favorites");
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness_de(&fx, WIDE, theme);
        select(&mut h, Side::LEFT, &["src", "photos"]);
        let items = [
            (local_path(&fx, "src"), true),
            (local_path(&fx, "photos"), true),
        ];
        let ctx = h.ctx.clone();
        let pos = between(&mut h, 0, true) + egui::vec2(10.0, 0.0);
        let p = payload(&mut h, &items, None);
        egui::DragAndDrop::set_payload(&ctx, p);
        h.hover_at(pos);
        h.run_steps(3);
        save_to(&mut h, &dir, &format!("favorites_drop_{tname}"));

        // Files only: refused.
        let p = payload(&mut h, &[(local_path(&fx, "notes.txt"), false)], None);
        egui::DragAndDrop::set_payload(&ctx, p);
        h.hover_at(pos);
        h.run_steps(3);
        save_to(&mut h, &dir, &format!("favorites_drop_files_{tname}"));
        egui::DragAndDrop::clear_payload(&ctx);

        // A duplicate flashes.
        let now = h.ctx.input(|i| i.time);
        app(&mut h).fav_flash = Some((0, Some(now - 0.22)));
        h.hover_at(egui::pos2(700.0, 400.0));
        h.step();
        save_to(&mut h, &dir, &format!("favorites_flash_{tname}"));
    }
}

/// "Remember password" puts the password into the (fake) Keychain; the
/// favorite and favorites.json only say so. Opening the favorite reads it
/// back, removing the favorite deletes the item.
#[test]
fn remembered_passwords_live_in_the_keychain() {
    use crate::secrets::SecretStore as _;
    let fx = fixture();
    let mut h = behaviour(&fx);
    let store = crate::secrets::MemorySecrets::new();
    let file = fx.root.join("favorites-keychain.json");
    {
        let a = app(&mut h);
        a.secrets = store.clone();
        a.favorites_path = Some(file.clone());
    }
    let mut cfg = manta_core::ConnectionConfig {
        name: "Staging".into(),
        protocol: manta_core::Protocol::Sftp,
        host: "staging.example.com".into(),
        port: 22,
        username: "deploy".into(),
        auth: manta_core::Auth::Password("correct horse".into()),
        initial_path: None,
        s3: None,
        container: None,
        ftp: None,
    };
    // The connect dialog with "Add to favorites" + "Remember password".
    let a = app(&mut h);
    let action = a.actions.next_id();
    let request = crate::worker::ConnectRequest {
        config: cfg.clone(),
        favorite: Some(cfg.clone()),
        side: Side::Left,
        action,
        keychain: false,
    };
    let fs: manta_core::SharedVfs = std::sync::Arc::new(manta_vfs::LocalFs::new());
    a.on_connected(request, Ok((fs, VfsPath::new(&fx.root.to_string_lossy()))));
    h.run_steps(2);

    let account = "sftp://deploy@staging.example.com:22";
    assert_eq!(
        store.get(account).unwrap().as_deref(),
        Some("correct horse")
    );
    let i = app(&mut h).favorites.items.len() - 1;
    let Favorite::Connection { config, keychain } = app(&mut h).favorites.items[i].clone() else {
        unreachable!()
    };
    assert!(keychain, "marked as stored in the Keychain");
    assert_eq!(
        config.auth,
        manta_core::Auth::Password(String::new()),
        "no password in memory"
    );
    let json = std::fs::read_to_string(&file).unwrap();
    assert!(!json.contains("correct horse"), "{json}");
    assert!(json.contains("\"keychain\": true"), "{json}");

    // Editing shows the stored secret as such; an empty field keeps it.
    let ctx = h.ctx.clone();
    app(&mut h).handle_action(&ctx, crate::app::Action::EditFavorite(i));
    let Some(Dialog::Connect(form)) = &app(&mut h).dialog else {
        panic!("connect dialog")
    };
    assert!(form.stored_secret && form.remember_password && form.password.is_empty());
    app(&mut h).dialog = None;

    // The Keychain refuses: nothing is saved, certainly not in plain text.
    store
        .fail_writes
        .store(true, std::sync::atomic::Ordering::Relaxed);
    cfg.host = "other.example.com".into();
    let a = app(&mut h);
    let action = a.actions.next_id();
    let request = crate::worker::ConnectRequest {
        config: cfg.clone(),
        favorite: Some(cfg.clone()),
        side: Side::Left,
        action,
        keychain: false,
    };
    let fs: manta_core::SharedVfs = std::sync::Arc::new(manta_vfs::LocalFs::new());
    a.on_connected(request, Ok((fs, VfsPath::new(&fx.root.to_string_lossy()))));
    let json = std::fs::read_to_string(&file).unwrap();
    assert!(!json.contains("correct horse"), "{json}");
    let last = app(&mut h).favorites.items.last().cloned().unwrap();
    assert!(matches!(
        last,
        Favorite::Connection {
            keychain: false,
            ..
        }
    ));
    store
        .fail_writes
        .store(false, std::sync::atomic::Ordering::Relaxed);

    // Removing the favorite deletes its Keychain item.
    app(&mut h).handle_action(&ctx, crate::app::Action::RemoveFavorite(i));
    assert_eq!(store.get(account).unwrap(), None);
    assert!(store.accounts().is_empty());
}
