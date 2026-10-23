//! Hello Rust: a minimal Manta plugin in Rust.
//!
//! How it talks to Manta (see docs/plugins.md):
//! * Manta -> plugin: one JSON object per line on stdin. The
//!   `manta-plugin-protocol` crate parses them into [`HostMessage`]s.
//! * plugin -> Manta: one JSON object per line on stdout, written with
//!   `out.send(&PluginMessage::…)` (one line, flushed right away).
//! * stderr is the log (Settings › Plugins › "Show log"): use `eprintln!`.
//!   NEVER `println!` anything: stdout is only for protocol messages.
//!
//! What it does:
//! * "Say Hello" in the command palette: a toast.
//! * "Count Selection" in the file list's context menu: a toast with the
//!   number of selected items.
//! * a chip in the path bar with the number of items in local folders.
//! * texts in English or German, following Manta's language.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use manta_plugin_protocol::{
    run, Chip, CommandContext, CommandDecl, HostMessage, Out, Pane, PaneState, PluginMessage,
    ToastLevel, Tone,
};

// ------------------------------------------------------------ language

/// The languages this plugin has texts for. Manta sends its UI language in
/// `init` ("en", "de", "zh-Hans", …) and `locale_changed`.
#[derive(Clone, Copy, PartialEq)]
enum Lang {
    En,
    De,
}

impl Lang {
    /// "de-DE" -> German; anything we don't translate -> English.
    fn from_locale(code: &str) -> Lang {
        match code.split(['-', '_']).next().unwrap_or("") {
            "de" => Lang::De,
            _ => Lang::En,
        }
    }

    fn say_hello(self) -> &'static str {
        match self {
            Lang::En => "Say Hello",
            Lang::De => "Hallo sagen",
        }
    }

    fn count_selection(self) -> &'static str {
        match self {
            Lang::En => "Count Selection",
            Lang::De => "Auswahl zählen",
        }
    }

    fn hello(self, folder: &str) -> String {
        match self {
            Lang::En => format!("Hello from Rust! You're in “{folder}”."),
            Lang::De => format!("Hallo aus Rust! Du bist in „{folder}“."),
        }
    }

    fn selected(self, n: usize) -> String {
        match (self, n) {
            (Lang::En, 1) => "1 item selected".into(),
            (Lang::En, n) => format!("{n} items selected"),
            (Lang::De, 1) => "1 Objekt ausgewählt".into(),
            (Lang::De, n) => format!("{n} Objekte ausgewählt"),
        }
    }

    fn items(self, n: usize) -> String {
        match (self, n) {
            (Lang::En, 1) => "1 item".into(),
            (Lang::En, n) => format!("{n} items"),
            (Lang::De, 1) => "1 Objekt".into(),
            (Lang::De, n) => format!("{n} Objekte"),
        }
    }

    fn items_tooltip(self) -> &'static str {
        match self {
            Lang::En => "Items in this folder, counted by Hello Rust",
            Lang::De => "Objekte in diesem Ordner, gezählt von Hello Rust",
        }
    }
}

// ------------------------------------------------------------ plugin

struct Plugin {
    lang: Lang,
    /// The last state of every pane, so we can re-send the chips in the
    /// new language after `locale_changed`.
    panes: BTreeMap<Pane, PaneState>,
}

impl Plugin {
    /// Commands with titles in the current language. Ids never change.
    fn register_commands(&self, out: &mut Out) -> io::Result<()> {
        let command = |id: &str, title: &str, context| CommandDecl {
            id: id.into(),
            title: title.into(),
            shortcut: None,
            context,
        };
        out.send(&PluginMessage::RegisterCommands {
            commands: vec![
                command("say-hello", self.lang.say_hello(), CommandContext::Palette),
                // The palette and the file list's right-click menu.
                command(
                    "count-selection",
                    self.lang.count_selection(),
                    CommandContext::FileContextMenu,
                ),
            ],
        })
    }

    /// The item-count chip for one pane (only for local folders).
    fn update_chips(&mut self, state: PaneState, out: &mut Out) -> io::Result<()> {
        // `local_path` is None for SFTP/FTP/S3/… folders: we can't read those.
        if let Some(local) = &state.local_path {
            match std::fs::read_dir(local) {
                Ok(entries) => {
                    let chip = Chip {
                        text: self.lang.items(entries.count()),
                        tone: Tone::Neutral,
                        icon: Some("folder".into()),
                        tooltip: Some(self.lang.items_tooltip().into()),
                        command: None,
                    };
                    // `dir` must be the folder the pane shows (`path`); Manta
                    // drops chips for a folder the user has already left.
                    out.send(&PluginMessage::SetChips {
                        pane: state.pane,
                        dir: state.path.clone(),
                        chips: vec![chip],
                    })?;
                }
                Err(e) => eprintln!("can't list {local}: {e}"),
            }
        }
        self.panes.insert(state.pane, state);
        Ok(())
    }

    fn handle(&mut self, msg: HostMessage, out: &mut Out) -> io::Result<()> {
        match msg {
            // Always the first message.
            HostMessage::Init { data_dir, panes, locale, .. } => {
                self.lang = Lang::from_locale(&locale);
                eprintln!("hello-rust started, data dir {data_dir}, locale {locale}");
                self.register_commands(out)?;
                for state in panes {
                    self.update_chips(state, out)?;
                }
            }
            // Re-send everything that has text.
            HostMessage::LocaleChanged { locale } => {
                self.lang = Lang::from_locale(&locale);
                self.register_commands(out)?;
                for state in std::mem::take(&mut self.panes).into_values() {
                    self.update_chips(state, out)?;
                }
            }
            HostMessage::DirectoryChanged { pane_state }
            | HostMessage::ListingRefreshed { pane_state } => self.update_chips(pane_state, out)?,
            // Forget it, so we don't send chips for it again.
            HostMessage::PaneClosed { pane } => {
                self.panes.remove(&pane);
            }
            // `pane` is the state of the active pane when the command ran.
            HostMessage::RunCommand { id, pane } => match id.as_str() {
                "say-hello" => {
                    let folder = Path::new(&pane.path)
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "/".into());
                    out.send(&PluginMessage::toast(ToastLevel::Success, self.lang.hello(&folder)))?;
                }
                "count-selection" => {
                    let n = pane.selection.len();
                    out.send(&PluginMessage::toast(ToastLevel::Info, self.lang.selected(n)))?;
                }
                _ => {}
            },
            // `run` returns right after this: main() ends, the process exits.
            HostMessage::Shutdown => eprintln!("hello-rust: bye"),
            // Everything else (and message types newer than this crate).
            _ => {}
        }
        Ok(())
    }
}

fn main() -> io::Result<()> {
    let mut plugin = Plugin { lang: Lang::En, panes: BTreeMap::new() };
    // Reads stdin line by line until `shutdown` or EOF; lines it can't parse
    // are logged to stderr and skipped.
    run(|msg, out| plugin.handle(msg, out))
}
