//! English and German strings of the Project Info plugin (see
//! `manta_plugin::i18n`): `(key, English, German)`, carried over from
//! the Python plugin's `pi_i18n.py` (plural counts are `{count}`).

use manta_plugin::i18n::{Catalog, Entries, Lang};

mod es;
mod fr;
mod it;
mod ru;
mod zh;

pub static CATALOG: Catalog = Catalog::with(
    ENTRIES,
    &[
        (Lang::Zh, zh::ENTRIES),
        (Lang::Es, es::ENTRIES),
        (Lang::Fr, fr::ENTRIES),
        (Lang::It, it::ENTRIES),
        (Lang::Ru, ru::ENTRIES),
    ],
);

macro_rules! tr {
    ($($t:tt)*) => { manta_plugin::tr_with!(crate::i18n::CATALOG, $($t)*) };
}
pub(crate) use tr;

/// German texts that are the same as the English ones on purpose.
pub const SAME_IN_GERMAN: &[&str] = &["Workspace", "Finder", "in {path}"];

const ENTRIES: Entries = &[
    // Plugin
    ("plugin.name", "Project Info", "Projektinfo"),
    ("plugin.description", "Detects the project you're in (Laravel, Composer, npm/pnpm, Docker Compose, Cargo, Python, Make…) and runs its scripts in the terminal with one click.", "Erkennt das Projekt, in dem du bist (Laravel, Composer, npm/pnpm, Docker Compose, Cargo, Python, Make …), und führt seine Skripte mit einem Klick im Terminal aus."),
    ("cmd.run", "Run Project Script…", "Projektskript ausführen …"),
    ("cmd.panel", "Show Project Panel", "Projekt-Panel einblenden"),
    ("cmd.rerun", "Re-run Last Script", "Letztes Skript erneut ausführen"),
    ("cmd.show_scripts", "Show Scripts", "Skripte zeigen"),
    ("panel.project", "Project", "Projekt"),
    ("panel.picker", "Run Project Script", "Projektskript ausführen"),
    ("panel.confirm", "Run Script", "Skript ausführen"),
    ("chip.scripts.one", "{label} · {count} script", "{label} · {count} Skript"),
    ("chip.scripts.other", "{label} · {count} scripts", "{label} · {count} Skripte"),
    ("chip.inside", "Inside {names}", "Innerhalb von {names}"),
    ("chip.click", "Click to show or hide the Project panel", "Klicken, um das Projekt-Panel ein- oder auszublenden"),
    ("kind.project", "Project", "Projekt"),
    ("tip.asks_first", "Asks first: {why}", "Fragt vorher nach: {why}"),
    ("tip.needs_args", "Needs arguments: add them in the terminal.", "Braucht Argumente: Ergänze sie im Terminal."),
    ("tip.open_in_pane", "Open {path} in the pane", "{path} im Bereich öffnen"),
    ("section.workspace", "Workspace", "Workspace"),
    ("ws.packages.one", "{count} package", "{count} Paket"),
    ("ws.packages.other", "{count} packages", "{count} Pakete"),
    ("section.recent", "Recent", "Zuletzt ausgeführt"),
    ("section.inside", "Inside {name}", "Innerhalb von {name}"),
    ("ago.now", "just now", "gerade eben"),
    ("ago.min", "{n} min ago", "vor {n} Min."),
    ("ago.hours", "{n} h ago", "vor {n} Std."),
    ("ago.days", "{n} d ago", "vor {n} T."),
    ("header.on_branch", "on {branch}", "auf {branch}"),
    ("header.detached", "{commit} (detached)", "{commit} (losgelöst)"),
    ("btn.vscode", "Open in VS Code", "In VS Code öffnen"),
    ("btn.finder", "Finder", "Finder"),
    ("btn.go_root", "Go to Root", "Zum Stammordner"),
    ("btn.close", "Close", "Schließen"),
    ("btn.cancel", "Cancel", "Abbrechen"),
    ("btn.run", "Run", "Ausführen"),
    ("panel.waiting", "Waiting for Manta…", "Warte auf Manta …"),
    ("panel.archive", "Inside an archive", "In einem Archiv"),
    ("panel.archive.note", "Project Info reads real folders. Extract the archive to see its scripts.", "Projektinfo liest echte Ordner. Entpacke das Archiv, um seine Skripte zu sehen."),
    ("panel.remote", "Remote folder", "Entfernter Ordner"),
    ("panel.remote.note", "Local folders only: Project Info reads the project files on this Mac.", "Nur lokale Ordner: Projektinfo liest die Projektdateien auf diesem Mac."),
    ("panel.no_project", "No project here", "Kein Projekt hier"),
    ("panel.no_project.note", "Open a folder with a package.json, composer.json, artisan, Cargo.toml, pyproject.toml, Makefile, justfile, Taskfile.yml or compose file, or a folder inside one.", "Öffne einen Ordner mit package.json, composer.json, artisan, Cargo.toml, pyproject.toml, Makefile, justfile, Taskfile.yml oder Compose-Datei oder einen Ordner darin."),
    ("panel.no_scripts", "No scripts found.", "Keine Skripte gefunden."),
    ("picker.no_project", "There is no project in this folder.", "In diesem Ordner gibt es kein Projekt."),
    ("picker.heading", "Run a script in {name}", "Skript in {name} ausführen"),
    ("picker.runs_in", "Runs in the terminal, in {path}", "Läuft im Terminal, in {path}"),
    ("confirm.heading", "Run “{name}”?", "„{name}“ ausführen?"),
    ("confirm.in", "in {path}", "in {path}"),
    ("toast.script_gone", "That script is gone (the project files changed)", "Dieses Skript gibt es nicht mehr (die Projektdateien haben sich geändert)"),
    ("toast.nothing_run", "Nothing run in {name} yet. Pick a script in the Project panel.", "In {name} wurde noch nichts ausgeführt. Wähle ein Skript im Projekt-Panel."),
    ("toast.no_project", "There is no project in this folder.", "In diesem Ordner gibt es kein Projekt."),
    ("toast.open_failed", "Couldn't open {what}: {error}", "{what} konnte nicht geöffnet werden: {error}"),
    ("toast.open_failed_short", "Couldn't open {what}", "{what} konnte nicht geöffnet werden"),
    ("toast.no_code", "VS Code's `code` command wasn't found", "Der Befehl „code“ von VS Code wurde nicht gefunden"),
    ("toast.no_env", "There is no .env in {path}", "In {path} gibt es keine .env"),
    ("what.vscode_missing", "VS Code (is it installed?)", "VS Code (ist es installiert?)"),
    ("what.folder", "the folder", "Der Ordner"),
    ("problem.unreadable", "can't be read", "kann nicht gelesen werden"),
    ("danger.tables", "This drops database tables. Data in them is lost.", "Löscht Datenbanktabellen. Die Daten darin gehen verloren."),
    ("danger.wipe", "This drops every table, view and type of the database.", "Löscht alle Tabellen, Views und Typen der Datenbank."),
    ("danger.volumes", "This also removes the containers' volumes (their data).", "Entfernt auch die Volumes der Container (ihre Daten)."),
    ("danger.rm", "This deletes files and folders.", "Löscht Dateien und Ordner."),
    ("danger.git", "This throws away work in Git.", "Verwirft Arbeit in Git."),
    ("danger.docker", "This removes Docker data.", "Entfernt Docker-Daten."),
    ("danger.dropdb", "This drops a database.", "Löscht eine Datenbank."),
    ("danger.publish", "This publishes a release; that can't be undone.", "Veröffentlicht ein Release. Das lässt sich nicht rückgängig machen."),
    ("danger.deploy", "This deploys: it changes things outside this Mac.", "Führt ein Deployment aus: Es ändert Dinge außerhalb dieses Macs."),
    ("danger.word", "“{name}” may delete data or build results.", "„{name}“ kann Daten oder Build-Ergebnisse löschen."),
    ("desc.install_deps", "Install the dependencies", "Abhängigkeiten installieren"),
    ("desc.serve", "Start the development server", "Entwicklungsserver starten"),
    ("desc.migrate", "Run the database migrations", "Datenbankmigrationen ausführen"),
    ("desc.migrate_fresh", "Drop all tables, migrate and seed", "Alle Tabellen löschen, migrieren und befüllen"),
    ("desc.tests", "Run the tests", "Tests ausführen"),
    ("desc.tinker", "Interactive shell (REPL)", "Interaktive Shell (REPL)"),
    ("desc.routes", "List the routes", "Routen auflisten"),
    ("desc.optimize_clear", "Clear every cache (config, routes, views …)", "Alle Caches leeren (Konfiguration, Routen, Views …)"),
    ("desc.queue", "Process jobs on the queue", "Jobs der Warteschlange abarbeiten"),
    ("desc.sail_up", "Start the Sail containers in the background", "Sail-Container im Hintergrund starten"),
    ("desc.sail_down", "Stop the Sail containers", "Sail-Container stoppen"),
    ("desc.compose_up", "Start the services in the background", "Dienste im Hintergrund starten"),
    ("desc.compose_down", "Stop and remove the containers", "Container stoppen und entfernen"),
    ("desc.compose_ps", "List the containers", "Container auflisten"),
    ("desc.compose_logs", "Follow the logs of every service", "Logs aller Dienste verfolgen"),
    ("desc.compose_down_v", "Stop and remove the containers and their volumes", "Container und ihre Volumes stoppen und entfernen"),
    ("desc.compose_logs_one", "Follow the logs of {service}", "Logs von {service} verfolgen"),
    ("desc.compile", "Compile", "Kompilieren"),
    ("desc.run_bin", "Run {name}", "{name} ausführen"),
    ("desc.lint", "Lint", "Linter ausführen"),
    ("desc.pip_install", "Install the requirements", "Abhängigkeiten aus requirements.txt installieren"),
    ("desc.pip_install_into", "Install the requirements into {venv}", "Abhängigkeiten in {venv} installieren"),
    ("desc.activate", "Activate the virtual environment in the terminal", "Virtuelle Umgebung im Terminal aktivieren"),
    ("detail.needs_args", "{detail} (needs arguments)", "{detail} (braucht Argumente)"),
    ("tag.compose_services.one", "Compose · {count} service", "Compose · {count} Dienst"),
    ("tag.compose_services.other", "Compose · {count} services", "Compose · {count} Dienste"),
    ("tag.cargo_workspace", "Cargo workspace", "Cargo-Workspace"),
    ("info.members.one", "{count} member", "{count} Mitglied"),
    ("info.members.other", "{count} members", "{count} Mitglieder"),
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use manta_plugin::i18n::{check, with_lang, Lang};

    use super::*;

    #[test]
    fn catalog_is_complete() {
        let p = check::catalog_problems(&CATALOG, SAME_IN_GERMAN);
        assert!(p.is_empty(), "{}", p.join("\n"));
    }

    #[test]
    fn every_used_key_exists() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let m = check::missing_keys(&CATALOG, &src, 60);
        assert!(m.is_empty(), "{}", m.join("\n"));
    }

    #[test]
    fn no_hard_coded_ui_strings() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let hits = check::hard_coded_strings(&src, &[], &[]);
        assert!(hits.is_empty(), "{}", hits.join("\n"));
    }

    #[test]
    fn lookup_and_plural() {
        with_lang(Lang::De, || {
            assert_eq!(
                tr!("chip.scripts", count = 1, label = "npm"),
                "npm · 1 Skript"
            );
            assert_eq!(tr!("ws.packages", count = 1234), "1.234 Pakete");
            assert_eq!(
                tr!("confirm.heading", name = "deploy"),
                "„deploy“ ausführen?"
            );
        });
        with_lang(Lang::En, || {
            assert_eq!(tr!("ws.packages", count = 1234), "1,234 packages")
        });
    }
}
