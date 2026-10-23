# Your Own Plugins {#eigene-plugins}

You can extend Manta with plugins of your own – in any programming language. This chapter explains what a plugin can do, how to install, update and find one and what to watch out for. The technical guide is in the developer documentation.

## What plugins can do {#moeglichkeiten}

- Show **badges** next to file names (like the Git badges).
- Show **chips** in the path bar (like the branch or the project).
- Offer **commands** for the command palette and the context menu, with a shortcut if you like.
- Show **panels** below a pane, in the right sidebar or as a dialog.
- Show **items in the status bar**, with an icon, a small meter and a warning colour (like [System Load](14-plugins.md#systemauslastung)).
- Show **notices**.

A plugin is a folder with a `plugin.toml` file and a program (a Python script, for example). Manta starts the program and exchanges messages with it (JSON lines over stdin/stdout). A `.mantaplugin` file is simply that folder as a zip archive.

## Install a plugin {#installieren}

1. Open **Settings** (**⌘,**) → **Plugins**.
2. Click **Install Plugin…** and choose **From a File (.mantaplugin, .zip)…** or **From a Folder…** (the folder with `plugin.toml`).
3. Before anything is installed, Manta shows an overview: name, version, author, description, **What it says it does** (for example "Reads your files", "Changes or deletes files", "Uses the network", "Runs other programs", "Runs commands in the Terminal"), the command it **Runs**, where it comes **From**, a link to the **Source code**, the **Licence** and the size. Below that, a warning that Manta doesn't check plugins.
4. Click **Install**. The plugin is then listed under **Installed** – still switched off.

![Overview before installing a plugin](images/plugin-install.png)

> [!NOTE]
> What a plugin says it does is declared by its author. Manta can't check or enforce it.

### Turn it on {#einschalten}

Click **Turn On** on the plugin's card. The first time, Manta asks you to confirm and shows the author, version, permissions, the command and the folder it runs in once more.

![Confirmation when turning on an external plugin](images/plugin-confirm.png)

If you downloaded the plugin from the internet, macOS keeps its files in quarantine, and Gatekeeper would block programs inside it. The confirmation says so: only when you turn the plugin on does Manta remove the quarantine flag from its files so its programs can run – never silently.

More buttons on a plugin's card: **Reload**, **Show Log**, **Show Folder**, **Uninstall**. At the top: **Open Plugins Folder** and **Reload** (looks for added, changed or removed plugins).

## Update and uninstall {#aktualisieren}

- **Update**: install the new version the same way, from a file or a folder. If it has the same id as an installed plugin, the overview says "Update …?" and shows the version that will be replaced under **Installed**. After you confirm, Manta replaces the plugin; whether it was on or off is kept.
- **Uninstall**: click **Uninstall** on the plugin's card. Manta deletes the plugin's folder; its data folder is kept.

## Find plugins {#entdecken}

In **Settings** → **Plugins**, click **Browse Plugins** at the top right (or choose **Browse Plugins…** in the command palette). This opens the plugin directory at [manta-filemanager.com/plugins](https://manta-filemanager.com/plugins/). It lists the built-in plugins and plugins from the community. Each entry has a link to its source code and a SHA-256 checksum of the download. Download the `.mantaplugin` file and install it as described [above](#installieren).

## Security {#sicherheit}

> [!WARNING]
> External plugins run as programs with **your user's permissions**: they can read, change and delete your files, start other programs and send data over the network. Manta doesn't check plugins and doesn't sandbox them – not even those from the plugin directory. Only install plugins you trust, and read the code of plugins from the internet before you turn them on.

That's why Manta shows an overview before installing, new plugins start switched off, and Manta asks the first time you turn one on.

## When a plugin doesn't run {#probleme}

- The card shows **Error** and the reason, for example when `python3` is missing.
- **Show Log** shows the plugin's last messages.
- If a plugin crashes, Manta restarts it – at most three times a minute. After that it stays in the error state until you click **Reload**.
- If the plugins folder contains two folders with the same plugin id, the second shows **Can't load** and a message. Remove one of them.

## Where plugins live {#speicherorte}

| What | Where |
|---|---|
| Installed plugins | `~/Library/Application Support/Manta/plugins/<id>/` |
| A plugin's data and log | `~/Library/Application Support/Manta/plugin-data/<id>/` (log: `plugin.log`) |
| On/off state | `~/Library/Application Support/Manta/plugins.json` |

Uninstalling deletes the plugin's folder; its data folder is kept.

## Write a plugin yourself {#schreiben}

The quickest start is the **Hello Python** example (`examples/plugins/hello-python` in the source code, commented line by line, Python standard library only). You can also install it from **Settings** → **Plugins** → **Available**. There are templates for Node.js, shell (bash + jq) and Rust too (`examples/plugins/`); for Rust there is also the `manta-plugin-protocol` crate.

Guide, protocol reference and examples: [Writing Manta plugins (docs/plugins.md)](../../plugins.md).

Plugins can speak all of Manta's languages: Manta tells them the current language, and `plugin.toml` can carry a translated name and description (`[localized.de]`, `[localized.fr]` and so on).

## Develop and test {#entwickeln}

- **Reload Plugins** (command palette) restarts all running external plugins and scans the plugins folder again.
- **Developer mode**: start Manta from Terminal with
  `/Applications/Manta.app/Contents/MacOS/manta --plugin-dev ~/code/my-plugin`
  (repeatable, or set `MANTA_PLUGIN_DEV=dir1:dir2`). For this session the plugin runs straight from that folder – trusted, switched on and tagged **Development**. An installed copy with the same id steps aside meanwhile; nothing is saved.
- **Test without Manta**: `python3 scripts/plugin-test.py path/to/plugin`.
- **Log**: **Show Log** on the card, or the file `~/Library/Application Support/Manta/plugin-data/<id>/plugin.log`.

More: [Develop and debug (docs/plugins.md)](../../plugins.md#develop-and-debug).

## Publish a plugin {#veroeffentlichen}

Put the plugin in a public repository with an OSI-approved licence, attach a `.mantaplugin` file to a release and open a pull request that adds it to `plugins/registry.json`. It then shows up under **Browse Plugins**. Details: [Publishing a plugin (docs/plugins.md)](../../plugins.md#publishing-a-plugin).
