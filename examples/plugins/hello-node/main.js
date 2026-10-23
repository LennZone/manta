#!/usr/bin/env node
// Hello Node: a minimal Manta plugin in plain Node.js (no npm packages).
//
// How it talks to Manta (see docs/plugins.md):
//   * Manta -> plugin: one JSON object per line on stdin
//     (`init`, `directory_changed`, `run_command`, `shutdown`, ...).
//   * plugin -> Manta: one JSON object per line on stdout
//     (`register_commands`, `set_chips`, `toast`, ...).
//   * stderr is the log (Settings › Plugins › "Show log").
//     NEVER print anything but protocol JSON to stdout: no console.log()!
//
// What it does:
//   * "Say Hello" in the command palette: a toast.
//   * "Count Selection" in the file list's context menu: a toast with the
//     number of selected items.
//   * a chip in the path bar with the number of items in local folders.
//   * texts in English or German, following Manta's language.
"use strict";

const fs = require("fs");
const path = require("path");
const readline = require("readline");

// ------------------------------------------------------------ language

// Manta sends its UI language in `init` ("en", "de", "zh-Hans", ...) and
// `locale_changed`. Unknown languages fall back to English.
const STRINGS = {
  en: {
    sayHello: "Say Hello",
    countSelection: "Count Selection",
    hello: "Hello from Node.js! You're in “{folder}”.",
    selected: (n) => (n === 1 ? "1 item selected" : `${n} items selected`),
    items: (n) => (n === 1 ? "1 item" : `${n} items`),
    itemsTooltip: "Items in this folder, counted by Hello Node",
  },
  de: {
    sayHello: "Hallo sagen",
    countSelection: "Auswahl zählen",
    hello: "Hallo aus Node.js! Du bist in „{folder}“.",
    selected: (n) => (n === 1 ? "1 Objekt ausgewählt" : `${n} Objekte ausgewählt`),
    items: (n) => (n === 1 ? "1 Objekt" : `${n} Objekte`),
    itemsTooltip: "Objekte in diesem Ordner, gezählt von Hello Node",
  },
};

let t = STRINGS.en;

function setLocale(code) {
  const lang = String(code || "en").split(/[-_]/)[0].toLowerCase(); // "de-DE" -> "de"
  t = STRINGS[lang] || STRINGS.en;
}

// ------------------------------------------------------------ talking

/** Send one message to Manta: a single JSON line on stdout. */
function send(message) {
  // One write per line. Node has no flush(): each write goes out as soon
  // as possible, in order (on macOS pipes asynchronously, see quit()).
  process.stdout.write(JSON.stringify(message) + "\n");
}

/** Exit once everything written so far has reached Manta. */
function quit() {
  process.stdout.write("", () => process.exit(0));
}

/** Log to stderr (Settings › Plugins › Show log). */
function log(text) {
  process.stderr.write(text + "\n");
}

// ------------------------------------------------------------ features

// The last state of every pane we heard about, so we can re-send the
// chips in the new language after `locale_changed`.
const panes = new Map();

/** Commands with titles in the current language. Ids never change. */
function registerCommands() {
  send({
    type: "register_commands",
    commands: [
      { id: "say-hello", title: t.sayHello, context: "palette" },
      // "file_context_menu": the palette and the file list's right-click menu.
      { id: "count-selection", title: t.countSelection, context: "file_context_menu" },
    ],
  });
}

/** The item-count chip for one pane (only for local folders). */
function updateChips(pane) {
  panes.set(pane.pane, pane);
  // `local_path` is null for SFTP/FTP/S3/... folders: we can't read those.
  if (!pane.local_path) return;
  let count;
  try {
    count = fs.readdirSync(pane.local_path).length;
  } catch (e) {
    log(`can't list ${pane.local_path}: ${e.message}`);
    return;
  }
  // `dir` must be the folder the pane shows (`path`); Manta drops chips
  // for a folder the user has already left.
  send({
    type: "set_chips",
    pane: pane.pane,
    dir: pane.path,
    chips: [{ text: t.items(count), icon: "folder", tone: "neutral", tooltip: t.itemsTooltip }],
  });
}

function runCommand(id, pane) {
  // `pane` is the state of the active pane when the command ran.
  if (id === "say-hello") {
    const folder = path.basename(pane.path) || "/";
    send({ type: "toast", level: "success", text: t.hello.replace("{folder}", folder) });
  } else if (id === "count-selection") {
    const n = (pane.selection || []).length;
    send({ type: "toast", level: "info", text: t.selected(n) });
  }
}

// ------------------------------------------------------------ events

function handle(msg) {
  switch (msg.type) {
    case "init": // always the first message
      setLocale(msg.locale);
      log(`hello-node started, data dir ${msg.data_dir}, locale ${msg.locale}`);
      registerCommands();
      for (const pane of msg.panes || []) updateChips(pane);
      break;
    case "locale_changed": // re-send everything that has text
      setLocale(msg.locale);
      registerCommands();
      for (const pane of panes.values()) updateChips(pane);
      break;
    case "directory_changed":
    case "listing_refreshed":
      updateChips(msg.pane_state);
      break;
    case "pane_closed": // forget it, so we don't send chips for it again
      panes.delete(msg.pane);
      break;
    case "run_command":
      runCommand(msg.id, msg.pane);
      break;
    case "shutdown": // exit within 500 ms
      quit();
      break;
    // Unknown types: ignore them. Newer Manta versions send more.
  }
}

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on("line", (line) => {
  if (!line.trim()) return;
  try {
    handle(JSON.parse(line));
  } catch (e) {
    // A bug in one handler shouldn't kill the plugin.
    log(`error handling ${line.slice(0, 200)}: ${e.stack || e}`);
  }
});
rl.on("close", quit); // stdin closed (EOF): Manta is gone
