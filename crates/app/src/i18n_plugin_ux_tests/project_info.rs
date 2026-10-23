//! German renders: Project Info (see mod.rs). Chips, the panel for the
//! Laravel and Node projects, the confirm sheet and the script picker.

use super::super::*;
use crate::i18n::{self, Locale};

/// Like `project_info_harness`, but the plugin starts in German and the
/// app is in German.
fn german_harness(fx: &Fixture, theme: Theme, config: &Path) -> H {
    write_recent(fx, config);
    let config = config.to_path_buf();
    let mut h = harness_full(
        fx,
        WIDE,
        theme,
        true,
        Box::new(move |ctx: &egui::Context| {
            // The app has just set its (English) locale; plugins started
            // from this thread now work in German.
            manta_plugin::i18n::set_thread_lang(Some(manta_plugin::i18n::Lang::De));
            pi_host(ctx, &config)
        }),
    );
    i18n::set_locale(Locale::De);
    app(&mut h).navigate_path(
        Side::Left,
        VfsPath::new(&fx.root.join("acme-shop").to_string_lossy()),
    );
    app(&mut h).navigate_path(
        Side::Right,
        VfsPath::new(&fx.root.join("storefront-ui").to_string_lossy()),
    );
    settle(&mut h);
    wait_until(&mut h, "project chips", |h| {
        let chip = |side: PaneId| h.state().plugins.chips(side).iter().any(|(id, _)| id == PI);
        chip(PaneId::Left) && chip(PaneId::Right)
    });
    h
}

#[test]
fn render_project_info_german() {
    require_gpu!();
    let fx = projects_fixture();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let config = tempfile::tempdir().unwrap();
        let mut h = german_harness(&fx, theme, config.path());
        let chip = h
            .state()
            .plugins
            .chips(PaneId::Left)
            .into_iter()
            .find(|(id, _)| id == PI)
            .unwrap()
            .1;
        assert_eq!(chip.text, "Laravel · 28 Skripte");

        h.run_steps(3);
        let img = h.render().expect("render");
        let bar = image::imageops::crop_imm(&img, 0, 70, 2560, 110).to_image();
        bar.save(out_dir().join(format!("project_info_de_chips_{tname}.png")))
            .expect("save");

        app(&mut h)
            .plugins
            .set_panel_open(PI, "project", None, true);
        wait_until(&mut h, "laravel panel", |h| {
            pi_panel_has(h, "\"acme-shop\"") && pi_panel_has(h, "Zuletzt ausgeführt")
        });
        settle(&mut h);
        save(&mut h, &format!("project_info_de_laravel_{tname}"));

        app(&mut h).active = Side::Right;
        wait_until(&mut h, "node panel", |h| {
            pi_panel_has(h, "\"storefront-ui\"")
        });
        settle(&mut h);
        save(&mut h, &format!("project_info_de_node_{tname}"));

        let state = app(&mut h).pane_state(Side::Right);
        let action = format!(
            "run\t{}\tnode/clean",
            fx.root.join("storefront-ui").display()
        );
        app(&mut h)
            .plugins
            .panel_action(PI, "project", &action, &state);
        wait_until(&mut h, "confirm sheet", |h| {
            h.state().plugins.is_panel_open(PI, "confirm", None)
        });
        settle(&mut h);
        save(&mut h, &format!("project_info_de_confirm_{tname}"));
        app(&mut h)
            .plugins
            .set_panel_open(PI, "confirm", None, false);

        app(&mut h).active = Side::Left;
        wait_until(&mut h, "laravel again", |h| {
            pi_panel_has(h, "\"acme-shop\"")
        });
        let state = app(&mut h).pane_state(Side::Left);
        app(&mut h)
            .plugins
            .run_command(&format!("{PI}:run"), &state);
        wait_until(&mut h, "picker", |h| {
            h.state().plugins.is_panel_open(PI, "picker", None)
                && pi_panel_has_in(h, "picker", "Skript in acme-shop")
        });
        settle(&mut h);
        save(&mut h, &format!("project_info_de_picker_{tname}"));
        app(&mut h).plugins.shutdown(Duration::from_secs(1));
        i18n::set_thread_locale(None);
        manta_plugin::i18n::set_thread_lang(None);
    }
}
