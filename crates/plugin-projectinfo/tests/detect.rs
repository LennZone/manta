//! Project detection against the fixture projects, ported from the Python
//! plugin's `tests/test_detect.py`, `test_parsers.py`, `test_toml.py` (now
//! the real `toml` crate) and the detection part of `test_i18n.py`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use manta_plugin::i18n::{with_lang, Lang};
use manta_plugin_projectinfo::detect::{self, Project, Script};
use manta_plugin_projectinfo::parsers as P;
use manta_plugin_projectinfo::value::{self, V};

fn fixtures() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .to_string_lossy()
        .into_owned()
}

fn fx(rel: &str) -> String {
    format!("{}/{rel}", fixtures())
}

fn find(rel: &str) -> Vec<Project> {
    with_lang(Lang::En, || {
        detect::find_projects(&fx(rel), Some(&fixtures()), "/nonexistent-home")
    })
}

fn find_in(dir: &Path) -> Vec<Project> {
    let d = dir.to_string_lossy();
    with_lang(Lang::En, || {
        detect::find_projects(&d, Some(&d), "/nonexistent-home")
    })
}

fn scripts(p: &Project, section: &str) -> Vec<Script> {
    p.section(section)
        .unwrap_or_else(|| panic!("no section {section}"))
        .scripts
        .clone()
}

fn names(p: &Project, section: &str) -> Vec<String> {
    scripts(p, section).into_iter().map(|s| s.name).collect()
}

fn by_name(p: &Project, section: &str) -> HashMap<String, Script> {
    scripts(p, section)
        .into_iter()
        .map(|s| (s.name.clone(), s))
        .collect()
}

fn write(path: &Path, text: &str) {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn tags(p: &Project) -> Vec<String> {
    p.tags.iter().map(|(_, t)| t.clone()).collect()
}

// ------------------------------------------------------------------ Laravel

fn laravel() -> Project {
    let projects = find("acme-shop");
    assert_eq!(projects.len(), 1);
    projects.into_iter().next().unwrap()
}

#[test]
fn laravel_kinds_and_chip() {
    let p = laravel();
    assert_eq!(p.name, "acme-shop");
    assert_eq!(p.root, fx("acme-shop"));
    assert_eq!(p.kinds, ["laravel", "composer", "node", "compose"]);
    assert_eq!(
        p.sections.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["laravel", "composer", "node", "compose"]
    );
    with_lang(Lang::En, || {
        assert_eq!(
            detect::chip_text(&p),
            format!("Laravel · {} scripts", p.count)
        )
    });
    assert_eq!(p.count, 28);
    assert_eq!(p.primary_icon, "rocket");
    assert_eq!(
        tags(&p),
        [
            "Laravel 11",
            "PHP ^8.2",
            "pnpm · Node 20",
            "Compose · 4 services"
        ]
    );
    assert!(p.info.contains(&("Laravel".into(), "11.9.2".into())));
    assert_eq!(p.env_file.as_deref(), Some(fx("acme-shop/.env").as_str()));
    assert!(p.has_env);
    assert!(p.problems.is_empty(), "{:?}", p.problems);
}

#[test]
fn laravel_section() {
    let p = laravel();
    assert_eq!(
        names(&p, "laravel"),
        [
            "serve",
            "migrate",
            "migrate:fresh --seed",
            "test",
            "tinker",
            "route:list",
            "optimize:clear",
            "queue:work",
            "sail up -d",
            "sail down"
        ]
    );
    let s = by_name(&p, "laravel");
    assert_eq!(s["serve"].command, "php artisan serve");
    assert_eq!(s["sail up -d"].command, "./vendor/bin/sail up -d");
    assert!(s["migrate:fresh --seed"].danger.is_some());
    assert!(s["migrate"].danger.is_none());
}

#[test]
fn composer_scripts_skip_event_hooks() {
    let p = laravel();
    assert_eq!(names(&p, "composer"), ["dev", "lint", "test", "install"]);
    let s = by_name(&p, "composer");
    assert_eq!(s["lint"].command, "composer run-script lint");
    assert_eq!(
        s["lint"].desc.as_deref(),
        Some("Fix the code style with Laravel Pint")
    );
    assert!(
        s["dev"].detail.contains("npx concurrently"),
        "list bodies are joined"
    );
}

#[test]
fn node_uses_pnpm_from_the_lockfile() {
    let s = by_name(&laravel(), "node");
    assert_eq!(s["dev"].command, "pnpm run dev");
    assert_eq!(s["dev"].detail, "vite");
    assert_eq!(s["build"].detail, "vite build");
    assert_eq!(s["install"].command, "pnpm install");
}

#[test]
fn compose_services() {
    let s = by_name(&laravel(), "compose");
    assert_eq!(s["up -d"].command, "docker compose up -d");
    assert_eq!(s["logs -f mysql"].command, "docker compose logs -f mysql");
    assert!(s["down -v"].danger.is_some());
    assert!(s["down"].danger.is_none());
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), &target).unwrap();
        }
    }
}

#[test]
fn without_sail_and_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("shop");
    copy_tree(Path::new(&fx("acme-shop")), &root);
    std::fs::remove_dir_all(root.join("vendor")).unwrap();
    std::fs::remove_file(root.join("composer.lock")).unwrap();
    let p = &with_lang(Lang::En, || {
        detect::find_projects(
            &root.to_string_lossy(),
            Some(&tmp.path().to_string_lossy()),
            "/x",
        )
    })[0];
    assert!(!by_name(p, "laravel").contains_key("sail up -d"));
    assert_eq!(p.tags[0].1, "Laravel");
}

// ------------------------------------------------------------------ Node

#[test]
fn package_manager_field_wins() {
    let p = &find("storefront-ui")[0];
    with_lang(Lang::En, || {
        assert_eq!(detect::chip_text(p), "pnpm · 11 scripts")
    });
    assert_eq!(p.kinds, ["node"]);
    assert_eq!(
        p.tags,
        [("node".to_string(), "pnpm · Node >=20".to_string())]
    );
    let s = by_name(p, "node");
    assert!(s["clean"].danger.is_some());
    assert_eq!(s["test:e2e"].command, "pnpm run test:e2e");
}

#[test]
fn lockfiles() {
    for (lock, pm) in [
        (Some("pnpm-lock.yaml"), "pnpm"),
        (Some("yarn.lock"), "yarn"),
        (Some("bun.lockb"), "bun"),
        (Some("bun.lock"), "bun"),
        (Some("package-lock.json"), "npm"),
        (None, "npm"),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("package.json"),
            r#"{"scripts": {"go": "node ."}}"#,
        );
        if let Some(l) = lock {
            write(&tmp.path().join(l), "");
        }
        let p = &find_in(tmp.path())[0];
        assert_eq!(
            by_name(p, "node")["go"].command,
            format!("{pm} run go"),
            "{lock:?}"
        );
    }
}

#[test]
fn odd_script_names_are_quoted() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("package.json"),
        r#"{"scripts": {"build all": "x", "it's": "y"}}"#,
    );
    let s = by_name(&find_in(tmp.path())[0], "node");
    assert_eq!(s["build all"].command, "npm run 'build all'");
    assert_eq!(s["it's"].command, "npm run 'it'\"'\"'s'");
}

#[test]
fn broken_package_json_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    write(&tmp.path().join("package.json"), r#"{"scripts": "#);
    let p = &find_in(tmp.path())[0];
    assert!(
        p.problems
            .first()
            .is_some_and(|x| x.starts_with("package.json:")),
        "{:?}",
        p.problems
    );
}

// ------------------------------------------------------------------ nesting

#[test]
fn nearest_root_first() {
    let projects = find("monorepo/packages/ui/src");
    assert_eq!(
        projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["ui", "monorepo"]
    );
    let (ui, mono) = (&projects[0], &projects[1]);
    assert_eq!(ui.root, fx("monorepo/packages/ui"));
    assert_eq!(
        by_name(ui, "node")["build"].command,
        "yarn run build",
        "lockfile of the workspace root"
    );
    let links = &mono.section("node").unwrap().links;
    assert_eq!(
        links.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["@acme/api", "@acme/ui"]
    );
    assert_eq!(links[1].detail, "packages/ui");
}

#[test]
fn node_modules_and_vendor_are_not_projects() {
    let n = |rel| find(rel).into_iter().map(|p| p.name).collect::<Vec<_>>();
    assert_eq!(n("monorepo/node_modules/left-pad"), ["monorepo"]);
    assert_eq!(n("acme-shop/vendor/bin"), ["acme-shop"]);
}

#[test]
fn subfolder_finds_the_project() {
    assert_eq!(
        find("acme-shop/app/Http/Controllers")
            .into_iter()
            .map(|p| p.name)
            .collect::<Vec<_>>(),
        ["acme-shop"]
    );
}

#[test]
fn walk_stops_at_home_and_git_root() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = tmp.path().join("outer");
    let inner = outer.join("repo/sub");
    std::fs::create_dir_all(&inner).unwrap();
    write(&outer.join("Makefile"), "x:\n");
    write(&outer.join("repo/Makefile"), "y:\n");
    let n = |home: &Path| {
        detect::find_projects(&inner.to_string_lossy(), None, &home.to_string_lossy())
            .into_iter()
            .map(|p| p.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(n(tmp.path()), ["repo", "outer"]);
    // $HOME itself is never a project.
    assert_eq!(n(&outer), ["repo"]);
    // A Git repository root ends the walk; its branch is reported.
    write(
        &outer.join("repo/.git/HEAD"),
        "ref: refs/heads/feature/login\n",
    );
    let projects = detect::find_projects(
        &inner.to_string_lossy(),
        None,
        &tmp.path().to_string_lossy(),
    );
    assert_eq!(
        projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["repo"]
    );
    assert_eq!(projects[0].git_branch.as_deref(), Some("feature/login"));
    // A worktree-style .git file pointing elsewhere.
    std::fs::remove_dir_all(outer.join("repo/.git")).unwrap();
    write(&outer.join("gitdir/HEAD"), "0123456789abcdef\n");
    write(&outer.join("repo/.git"), "gitdir: ../gitdir\n");
    let projects = with_lang(Lang::En, || {
        detect::find_projects(
            &inner.to_string_lossy(),
            None,
            &tmp.path().to_string_lossy(),
        )
    });
    assert_eq!(
        projects[0].git_branch.as_deref(),
        Some("0123456 (detached)")
    );
}

#[test]
fn no_project() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(find_in(tmp.path()).is_empty());
}

// ------------------------------------------------------------------ Cargo

#[test]
fn cargo_workspace() {
    let p = &find("cargo-ws")[0];
    assert_eq!(p.kinds, ["cargo"]);
    assert_eq!(
        names(p, "cargo"),
        [
            "build",
            "test",
            "clippy",
            "run --bin acme",
            "run --bin migrate"
        ]
    );
    assert_eq!(
        by_name(p, "cargo")["run --bin acme"].command,
        "cargo run --bin acme"
    );
    let links = &p.section("cargo").unwrap().links;
    assert_eq!(
        links.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["acme-cli", "acme-core"]
    );
    assert!(p.info.contains(&("Workspace".into(), "2 members".into())));
    assert_eq!(tags(p), ["Cargo workspace"]);
}

#[test]
fn cargo_member_is_nearest() {
    let projects = find("cargo-ws/crates/core/src");
    assert_eq!(
        projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["core", "cargo-ws"]
    );
    assert!(
        !names(&projects[0], "cargo").contains(&"run".to_string()),
        "a library has nothing to run"
    );
}

#[test]
fn cargo_single_binary_and_excludes() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("Cargo.toml"),
        "[package]\nname = \"hi\"\nedition = \"2021\"\n",
    );
    write(&tmp.path().join("src/main.rs"), "fn main() {}\n");
    let p = &find_in(tmp.path())[0];
    assert_eq!(names(p, "cargo"), ["build", "test", "run", "clippy"]);
    assert_eq!(p.tags, [("cargo".to_string(), "Rust 2021".to_string())]);

    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\", \"old\"]\nexclude = [\"old\"]\n",
    );
    write(
        &tmp.path().join("a/Cargo.toml"),
        "[package]\nname = \"a\"\nautobins = false\n",
    );
    write(&tmp.path().join("a/src/main.rs"), "");
    write(
        &tmp.path().join("old/Cargo.toml"),
        "[package]\nname = \"old\"\n",
    );
    let p = &find_in(tmp.path())[0];
    assert_eq!(p.section("cargo").unwrap().links.len(), 1);
    assert_eq!(
        names(p, "cargo"),
        ["build", "test", "clippy"],
        "autobins = false: no binaries"
    );
}

#[test]
fn broken_cargo_toml_is_reported_with_its_line() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nedition = \n",
    );
    write(&tmp.path().join("Makefile"), "a:\n");
    let p = &find_in(tmp.path())[0];
    assert!(
        p.problems
            .first()
            .is_some_and(|x| x.starts_with("Cargo.toml: line 3")),
        "{:?}",
        p.problems
    );
}

// ------------------------------------------------------------------ tools

#[test]
fn make() {
    let p = &find("make-project")[0];
    let s = by_name(p, "make");
    assert_eq!(s["test"].command, "make test");
    assert_eq!(s["test"].detail, "Run the tests");
    assert_eq!(s["lint"].detail, "make lint");
    assert!(s["clean"].danger.is_some());
    assert!(s["deploy"].danger.is_some());
    assert_eq!(
        p.section("make").unwrap().detail.as_deref(),
        Some("Makefile")
    );
}

#[test]
fn just() {
    let s = by_name(&find("just-project")[0], "just");
    assert_eq!(s["build"].command, "just build");
    assert!(s["deploy"].needs_args);
    assert!(s["deploy"].detail.ends_with("(needs arguments)"));
    assert!(!s.contains_key("helper"));
}

#[test]
fn task() {
    let p = &find("task-project")[0];
    assert_eq!(names(p, "task"), ["default", "build", "test", "db:reset"]);
    let s = by_name(p, "task");
    assert_eq!(s["db:reset"].command, "task db:reset");
    assert!(s["db:reset"].danger.is_some());
    assert_eq!(s["build"].detail, "Build the app");
}

#[test]
fn python() {
    let p = &find("py-project")[0];
    assert_eq!(
        p.tags,
        [("python".to_string(), "Hatch · Python >=3.10".to_string())]
    );
    let s = by_name(p, "python");
    assert_eq!(s["acme-sync"].command, "hatch run acme-sync");
    assert_eq!(
        s["acme-sync"].desc.as_deref(),
        Some("acme-sync (acme_tools.sync:main)")
    );
    assert_eq!(s["test"].detail, "pytest {args:tests}");
    assert_eq!(s["cov"].detail, "coverage run -m pytest && coverage report");
    assert_eq!(s["docs:serve"].command, "hatch run docs:serve");
    assert_eq!(
        s["pip install -r requirements.txt"].command,
        ".venv/bin/pip install -r requirements.txt"
    );
    assert_eq!(s["activate .venv"].command, "source .venv/bin/activate");
    assert!(p
        .info
        .contains(&("venv".into(), ".venv (Python 3.12.4)".into())));
    assert_eq!(
        names(p, "python")[..2],
        ["acme-sync", "acme-report"],
        "file order"
    );
}

#[test]
fn poetry_pdm_and_plain_requirements() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("pyproject.toml"),
        "[tool.poetry]\nname = \"x\"\n[tool.poetry.scripts]\nserve = \"x.app:main\"\n",
    );
    write(&tmp.path().join("requirements.txt"), "flask\n");
    let p = &find_in(tmp.path())[0];
    let s = by_name(p, "python");
    assert_eq!(s["serve"].command, "poetry run serve");
    assert_eq!(
        s["pip install -r requirements.txt"].command,
        "python3 -m pip install -r requirements.txt"
    );
    assert_eq!(p.tags[0].1, "Poetry · Python");

    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("pyproject.toml"),
        "[project]\nname = \"y\"\n[tool.pdm.scripts]\n_hidden = \"x\"\nlint = {cmd = [\"ruff\", \"check\"]}\nstart = \"flask run\"\n",
    );
    let p = &find_in(tmp.path())[0];
    assert_eq!(names(p, "python"), ["lint", "start"]);
    assert_eq!(by_name(p, "python")["lint"].detail, "ruff check");
    assert_eq!(by_name(p, "python")["start"].command, "pdm run start");
}

// ------------------------------------------------------------------ danger

#[test]
fn destructive_flagged() {
    for (name, cmd) in [
        ("migrate:fresh --seed", "php artisan migrate:fresh --seed"),
        ("fresh", "php artisan migrate:fresh"),
        ("db", "php artisan db:wipe"),
        ("down -v", "docker compose down -v"),
        ("stop", "docker compose down --volumes --remove-orphans"),
        ("clean", "make clean"),
        ("reset", "./reset.sh"),
        ("db:reset", "task db:reset"),
        ("nuke", "x"),
        ("tidy", "rm -rf dist"),
        ("tidy", "rm -fr build"),
        ("undo", "git reset --hard HEAD~1"),
        ("scrub", "git clean -fdx"),
        ("release", "npm publish"),
        ("deploy", "./deploy.sh"),
        ("prune", "docker system prune -af"),
    ] {
        assert!(detect::destructive(name, cmd).is_some(), "{name} {cmd}");
    }
}

#[test]
fn destructive_not_flagged() {
    for (name, cmd) in [
        ("migrate", "php artisan migrate"),
        ("optimize:clear", "php artisan optimize:clear"),
        ("down", "docker compose down"),
        ("logs -f", "docker compose logs -f"),
        ("dev", "vite"),
        ("build", "vite build"),
        ("test", "vitest run"),
        ("lint", "eslint . --fix"),
        ("cleanup-css", "postcss"), // 'cleanup' is not 'clean'
        ("rm-one", "rm dist/a.js"),
        ("tinker", "php artisan tinker"),
    ] {
        assert!(detect::destructive(name, cmd).is_none(), "{name} {cmd}");
    }
}

#[test]
fn destructive_reasons_are_translated() {
    with_lang(Lang::De, || {
        assert_eq!(
            detect::destructive("x", "php artisan db:wipe").unwrap(),
            "Löscht alle Tabellen, Views und Typen der Datenbank."
        );
        assert_eq!(
            detect::destructive("nuke", "x").unwrap(),
            "„nuke“ kann Daten oder Build-Ergebnisse löschen."
        );
    });
}

// ------------------------------------------------------------------ watching

#[test]
fn signature_changes_when_a_manifest_changes_or_appears() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, sub) = (
        tmp.path().to_string_lossy().into_owned(),
        tmp.path().join("src"),
    );
    std::fs::create_dir_all(&sub).unwrap();
    let sub = sub.to_string_lossy().into_owned();
    let projects = detect::find_projects(&sub, Some(&root), "/x");
    let watch = detect::watch_list(&sub, &projects, Some(&root), "/x");
    assert!(watch.contains(&format!("{root}/package.json")));
    assert!(!watch.concat().contains("node_modules"));
    let sig = detect::signature(&watch);
    write(
        &tmp.path().join("package.json"),
        r#"{"scripts": {"a": "b"}}"#,
    );
    assert_ne!(detect::signature(&watch), sig);
    let projects = detect::find_projects(&sub, Some(&root), "/x");
    assert_eq!(names(&projects[0], "node"), ["a", "install"]);
    let watch = detect::watch_list(&sub, &projects, Some(&root), "/x");
    let sig = detect::signature(&watch);
    std::thread::sleep(Duration::from_millis(10));
    write(
        &tmp.path().join("package.json"),
        r#"{"scripts": {"a": "b", "c": "d"}}"#,
    );
    assert_ne!(detect::signature(&watch), sig);
    let projects = detect::find_projects(&sub, Some(&root), "/x");
    assert_eq!(names(&projects[0], "node"), ["a", "c", "install"]);
}

#[test]
fn detection_is_fast_with_a_huge_node_modules() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("package.json"),
        r#"{"scripts": {"a": "b"}}"#,
    );
    let nm = tmp.path().join("node_modules");
    for i in 0..300 {
        write(&nm.join(format!("pkg{i}/package.json")), "{}");
    }
    let root = tmp.path().to_string_lossy().into_owned();
    let inside = nm.join("pkg7").to_string_lossy().into_owned();
    let start = Instant::now();
    for _ in 0..20 {
        detect::find_projects(&inside, Some(&root), "/x");
    }
    assert!(
        start.elapsed() / 20 < Duration::from_millis(50),
        "{:?}",
        start.elapsed() / 20
    );
    assert_eq!(
        detect::find_projects(&inside, Some(&root), "/x")
            .iter()
            .map(|p| p.root.clone())
            .collect::<Vec<_>>(),
        [root]
    );
}

#[test]
fn detection_in_german() {
    let p = with_lang(Lang::De, || {
        detect::find_projects(&fx("acme-shop"), Some(&fixtures()), "/x").remove(0)
    });
    assert!(p
        .all_scripts()
        .any(|(_, s)| s.desc.as_deref() == Some("Tests ausführen")));
    with_lang(Lang::De, || {
        assert_eq!(detect::chip_text(&p), "Laravel · 28 Skripte")
    });
}

// ------------------------------------------------------------------ parsers

fn read(rel: &str) -> String {
    std::fs::read_to_string(fx(rel)).unwrap()
}

#[test]
fn makefile_fixture() {
    let got = P::parse_makefile(&read("make-project/Makefile"));
    assert_eq!(
        got.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
        [
            "help",
            "build",
            "test",
            "lint",
            "deps",
            "clean",
            "deploy",
            "docker-build",
            "docker-push"
        ]
    );
    let docs: HashMap<_, _> = got.into_iter().collect();
    assert_eq!(docs["help"].as_deref(), Some("Show this help"));
    assert_eq!(
        docs["build"].as_deref(),
        Some("Compile the app"),
        "## line above the rule"
    );
    assert_eq!(docs["test"].as_deref(), Some("Run the tests"));
    assert_eq!(docs["lint"], None);
    assert_eq!(
        docs["docker-push"].as_deref(),
        Some("Build or push the image")
    );
}

#[test]
fn makefile_skips_specials_patterns_variables_and_recipes() {
    let text = ".PHONY: all\n.DEFAULT_GOAL := all\nCC := gcc\nX ::= 1\nY != date\nZ += 2\n\
        export FOO := bar\nall: a.o\n\t@echo all: done\n%.o: %.c\n\tcc $<\n\
        $(BUILD)/x: y\nsrc/out.txt: in\nifeq ($(A),1)\nB = 2\nendif\n\
        dbl:: one\nnext: ; @echo inline\nlong \\\n  name: x\n";
    let got: Vec<String> = P::parse_makefile(text)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert_eq!(got, ["all", "dbl", "next", "long", "name"]);
}

#[test]
fn makefile_details() {
    assert_eq!(
        P::parse_makefile("build: CFLAGS += -g\nbuild:\n\tcc\n"),
        [("build".to_string(), None)]
    );
    let got: HashMap<_, _> = P::parse_makefile("## Doc for a\n\na:\n## Doc for b\nb:\n")
        .into_iter()
        .collect();
    assert_eq!(got["a"], None);
    assert_eq!(got["b"].as_deref(), Some("Doc for b"));
    assert_eq!(
        P::parse_makefile("define X\nfake: target\nendef\nreal:\n"),
        [("real".to_string(), None)]
    );
}

#[test]
fn justfile_fixture() {
    let got = P::parse_justfile(&read("just-project/justfile"));
    assert_eq!(
        got.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["default", "build", "serve", "deploy", "lint"]
    );
    let info: HashMap<_, _> = got.into_iter().map(|(r, d, n)| (r, (d, n))).collect();
    assert_eq!(info["default"], (Some("List the recipes".into()), false));
    assert_eq!(
        info["serve"],
        (Some("Serve on a port".into()), false),
        "default value: no args needed"
    );
    assert_eq!(
        info["deploy"],
        (Some("Deploy to an environment".into()), true)
    );
    assert_eq!(info["lint"], (None, false), "variadic * is optional");
}

#[test]
fn justfile_details() {
    let text = "set shell := [\"bash\", \"-c\"]\nx := 'y'\nexport A := '1'\nalias t := test\ntest:\n  echo\n";
    assert_eq!(P::parse_justfile(text), [("test".to_string(), None, false)]);
    assert_eq!(
        P::parse_justfile("push +targets:\n  echo\n"),
        [("push".to_string(), None, true)]
    );
}

#[test]
fn yaml_compose_services() {
    let doc = P::parse_yaml(&read("acme-shop/docker-compose.yml")).unwrap();
    let got: Vec<String> = P::yaml_children(&doc, "services")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(got, ["laravel.test", "mysql", "redis", "mailpit"]);
    let text = "version: '3'\n# top\nservices:\n  # the app\n  web:\n    image: nginx\n    ports:\n      - 80:80\n\n  'db':\n    image: postgres\nvolumes:\n  data:\n";
    let got: Vec<String> = P::yaml_children(&P::parse_yaml(text).unwrap(), "services")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(got, ["web", "db"]);
    assert!(P::yaml_children(&P::parse_yaml("volumes:\n  a:\n").unwrap(), "services").is_empty());
}

#[test]
fn yaml_taskfile_and_list() {
    let doc = P::parse_yaml(&read("task-project/Taskfile.yml")).unwrap();
    let got = P::yaml_children(&doc, "tasks");
    assert_eq!(
        got.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["default", "build", "test", "db:reset", "internal-helper"]
    );
    let field = |t: &str, f: &str| {
        got.iter()
            .find(|(n, _)| n == t)
            .and_then(|(_, fs)| fs.iter().find(|(k, _)| k == f))
            .map(|(_, v)| v.clone())
    };
    assert_eq!(field("build", "desc").as_deref(), Some("Build the app"));
    assert_eq!(field("test", "desc").as_deref(), Some("Run the tests"));
    assert_eq!(
        field("internal-helper", "internal").as_deref(),
        Some("true")
    );
    assert_eq!(
        field("build", "cmds"),
        None,
        "only scalar fields one level down"
    );
    let text = "packages:\n  - 'apps/*'\n  - \"packages/*\"  # libs\n  - '!**/test/**'\ncatalog:\n  x: 1\n";
    assert_eq!(
        P::yaml_list(&P::parse_yaml(text).unwrap(), "packages"),
        ["apps/*", "packages/*", "!**/test/**"]
    );
}

// ------------------------------------------------------------------ TOML

#[test]
fn toml_fixture_files() {
    let ws = value::toml(&read("cargo-ws/Cargo.toml")).unwrap();
    let get = |v: &V, path: &[&str]| -> V {
        path.iter()
            .fold(v.clone(), |acc, k| acc.get(k).cloned().unwrap_or(V::Null))
    };
    assert_eq!(
        get(&ws, &["workspace", "members"]),
        V::List(vec![V::Str("crates/*".into())])
    );
    assert_eq!(
        get(&ws, &["workspace", "dependencies", "serde", "features"]),
        V::List(vec![V::Str("derive".into())])
    );
    assert_eq!(get(&ws, &["profile", "release", "lto"]), V::Bool(true));
    let cli = value::toml(&read("cargo-ws/crates/cli/Cargo.toml")).unwrap();
    assert_eq!(
        get(&cli, &["package", "description"]),
        V::Str("The command line tool.".into())
    );
    assert_eq!(
        get(&cli, &["dependencies", "anyhow"]),
        V::Map(vec![("workspace".into(), V::Bool(true))])
    );
    let py = value::toml(&read("py-project/pyproject.toml")).unwrap();
    assert_eq!(
        get(&py, &["project", "description"]),
        V::Str("Tools for the \"Acme\" team".into())
    );
    assert_eq!(
        get(&py, &["tool", "hatch", "envs", "default", "scripts", "cov"]),
        V::List(vec![
            V::Str("coverage run -m pytest".into()),
            V::Str("coverage report".into())
        ])
    );
    // Keys keep their order.
    let keys: Vec<&str> = py
        .get("project")
        .unwrap()
        .map()
        .unwrap()
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "name",
            "version",
            "description",
            "requires-python",
            "dependencies",
            "scripts"
        ]
    );
}

#[test]
fn toml_bom_crlf_and_real_world_files() {
    let v = value::toml("\u{feff}a = 1\r\n[b]\r\nc = \"d\"\r\n").unwrap();
    assert_eq!(
        v,
        V::Map(vec![
            ("a".into(), V::Int(1)),
            ("b".into(), V::Map(vec![("c".into(), V::Str("d".into()))]))
        ])
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for rel in [
        "Cargo.toml",
        "crates/app/Cargo.toml",
        "crates/plugin/Cargo.toml",
    ] {
        let text = std::fs::read_to_string(root.join(rel)).unwrap();
        assert!(value::toml(&text).unwrap().is_map(), "{rel}");
    }
    assert!(value::toml("a = [1, 2\n").unwrap_err().starts_with("line "));
}

#[test]
fn path_helpers_match_python() {
    assert_eq!(detect::dirname("/a/b"), "/a");
    assert_eq!(detect::dirname("/a"), "/");
    assert_eq!(detect::dirname("/"), "/");
    assert_eq!(detect::normpath("/a//b/./c/../d/"), "/a/b/d");
    assert_eq!(detect::relpath("/a/b/c", "/a"), "b/c");
    assert_eq!(detect::relpath("/a", "/a/b"), "..");
    assert!(detect::fnmatch("pkg-1", "pkg-*"));
    assert!(detect::fnmatch("a", "[abc]"));
    assert!(!detect::fnmatch("d", "[!d]"));
    assert!(detect::fnmatch("x9", "x?"));
    assert!(!detect::fnmatch(".hidden", "x*"));
    assert_eq!(
        detect::short_path("/Users/me/Sites/a", "/Users/me", 48),
        "~/Sites/a"
    );
    assert_eq!(detect::short_path("/Users/me", "/Users/me", 48), "~");
}
