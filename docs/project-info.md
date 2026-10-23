# Project Info

The scripts of the project you are in, one click away. Project Info detects
the project the active pane shows (or a folder inside it) and turns its
scripts into commands that run visibly in Manta's terminal. Built into
Manta (Settings → Plugins → Project Info, on by default; scripts run in
the built-in **Terminal** plugin, so keep that on). It replaces the former
bundled Python plugin `project-info`: an installed copy is switched off at
startup and its `recent.json` is taken over. `Cargo.toml` and
`pyproject.toml` are read with a full TOML parser, compose files, Taskfiles
and `pnpm-workspace.yaml` with a full YAML parser.

## Use

- **Path-bar chip**: the kind of project and how many scripts it has, e.g.
  "Laravel · 28 scripts" or "pnpm · 11 scripts". Hover for the root and the
  detected tools; click to show or hide the panel.
- **Project panel** (right sidebar):
  - the project name, its root, the Git branch and tags such as
    "Laravel 11", "PHP ^8.2", "pnpm · Node 20", "Compose · 4 services";
  - **Open in VS Code**, **Finder**, **.env** (if there is one), and **Go to
    Root** when the pane is in a subfolder;
  - **Recent**: the last 5 scripts you ran in this project (kept in
    `recent.json` in the plugin's data folder);
  - one section per tool. Each row shows the script's name and a short
    preview of what it runs; hover for the full command, click to run it.
    Destructive scripts (marked with a warning sign) ask first;
  - a project inside another one (a package in a monorepo, a crate in a
    Cargo workspace) shows the nearest project first, then the outer one;
    workspace packages are listed and open in the pane when clicked.
- **Command palette** (⇧⌘P): **Run Project Script…** (a sheet with every
  script), **Show Project Panel** and **Re-run Last Script** (⌥⌘R; it asks
  again if the script is destructive).
- **Context menu** on a `package.json`, `composer.json`, `Makefile` (or any
  other manifest below): **Show Scripts**.

Scripts run in the terminal below the pane, in the project root (`cd` first),
as if you had typed them, so they show their output live and land in your
shell history. The panel and chip update when you navigate and when a
manifest changes (checked every second for the shown project). Remote
(SFTP/FTP) folders show "Local folders only".

## What is detected

The plugin walks up from the pane's folder and stops before `$HOME` or `/`,
after the root of the Git repository, and never looks inside
`node_modules` or `vendor` (a folder in there belongs to the project above).

| tool | files | commands |
|---|---|---|
| Laravel | `artisan` | `php artisan serve`, `migrate`, `migrate:fresh --seed` (asks first), `test`, `tinker`, `route:list`, `optimize:clear`, `queue:work`; `./vendor/bin/sail up -d` / `sail down` when Sail is installed. The Laravel version comes from `composer.lock` |
| Composer | `composer.json` | `composer run-script <name>` for each script (not the event hooks like `post-autoload-dump`; `scripts-descriptions` become tooltips), `composer install`. The PHP requirement is shown |
| Node | `package.json` | `<pm> run <script>` and `<pm> install`. The package manager comes from `packageManager`, else the nearest lockfile: `pnpm-lock.yaml` → pnpm, `yarn.lock` → yarn, `bun.lockb`/`bun.lock` → bun, else npm. Node version from `.nvmrc`, `.node-version` or `engines.node`. Workspaces from `workspaces` or `pnpm-workspace.yaml` |
| Docker Compose | `compose.yaml`, `compose.yml`, `docker-compose.yaml`, `docker-compose.yml` | `docker compose up -d`, `down`, `ps`, `logs -f`, `down -v` (asks first) and `logs -f <service>` per service |
| Rust | `Cargo.toml` | `cargo build`, `test`, `run` (one binary) or `run --bin <name>` (several, also from workspace members), `clippy`. Workspace members are listed |
| Python | `pyproject.toml`, `requirements.txt` | `[project.scripts]` (through `uv run`, `poetry run`, `pdm run`, `hatch run` or the venv), Poetry scripts, Hatch env scripts (`hatch run env:name`), PDM scripts, `pip install -r requirements.txt` (the venv's pip if there is one) and `source .venv/bin/activate` |
| Make | `Makefile`, `makefile`, `GNUmakefile` | `make <target>`; `.PHONY` and other special targets, pattern rules, targets with variables or paths are left out. `target: ## text` or a `## text` line above becomes the description |
| just | `justfile` | `just <recipe>`; private recipes (`_name`, `[private]`) are left out, comments and `[doc(…)]` are descriptions |
| Task | `Taskfile.yml` | `task <name>` with its `desc`; `internal: true` tasks are left out |

### Confirmation

A script asks before it runs when its command drops data
(`migrate:fresh/reset/refresh`, `db:wipe`, `down -v`/`--volumes`, `rm -rf`,
`git clean`, `git reset --hard`, `docker … prune`, `dropdb`, `npm publish` …)
or its name contains clean, reset, wipe, purge, destroy, drop, nuke, prune,
fresh, deploy, publish or release.

### Language

The plugin follows Manta's language (English or German, Settings →
General → Language): command and panel titles, the chip, the panel, the
confirm sheet, toasts and the built-in script descriptions switch live.
Project data stays as it is: script names, commands, descriptions from
`composer.json`, Makefiles and the like.
