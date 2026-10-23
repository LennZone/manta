//! Keyboard shortcut strings as used in [`crate::CommandSpec::shortcut`]:
//! modifier symbols (`⌃ ⌥ ⇧ ⌘`, or the words `ctrl`/`control`, `alt`/
//! `option`/`opt`, `shift`, `cmd`/`command`, optionally joined with `+`)
//! followed by one key: a letter, digit, punctuation including the
//! backtick, or a name like `Space`, `Enter`, `Tab`, `Esc`, `F5`, `Up`.
//!
//! Examples: `"⌃`"`, `"⇧⌘G"`, `"⌥⌘T"`, `"ctrl+shift+k"`, `"⌘ + F5"`.

use egui::{Event, InputState, Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub modifiers: Modifiers,
    pub key: Key,
}

fn key_from(token: &str) -> Option<Key> {
    let t = token.trim();
    let alias = match t {
        "↩" | "⏎" | "Return" | "return" | "enter" => Some(Key::Enter),
        "⇥" | "tab" => Some(Key::Tab),
        "⎋" | "Esc" | "esc" | "escape" => Some(Key::Escape),
        "⌫" | "backspace" => Some(Key::Backspace),
        "⌦" | "delete" | "Del" | "del" => Some(Key::Delete),
        "↑" | "up" => Some(Key::ArrowUp),
        "↓" | "down" => Some(Key::ArrowDown),
        "←" | "left" => Some(Key::ArrowLeft),
        "→" | "right" => Some(Key::ArrowRight),
        "␣" | "space" => Some(Key::Space),
        "pageup" | "PgUp" => Some(Key::PageUp),
        "pagedown" | "PgDn" => Some(Key::PageDown),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        _ => None,
    };
    if alias.is_some() {
        return alias;
    }
    if let Some(k) = Key::from_name(t) {
        return Some(k);
    }
    // Single letters in either case, "f5" for F5.
    let mut chars = t.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphabetic() {
            return Key::from_name(&c.to_ascii_uppercase().to_string());
        }
    }
    let upper = t.to_ascii_uppercase();
    if upper.starts_with('F') {
        return Key::from_name(&upper);
    }
    // "Space", "Enter", … in other cases.
    let mut cap = t.to_ascii_lowercase();
    if let Some(f) = cap.get_mut(0..1) {
        f.make_ascii_uppercase();
    }
    Key::from_name(&cap)
}

/// Parse a shortcut string. The key must be the last part.
pub fn parse(s: &str) -> Result<Shortcut, String> {
    let mut mods = Modifiers::NONE;
    let mut rest = s.trim();
    if rest.is_empty() {
        return Err("empty shortcut".into());
    }
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '+');
        let lower = rest.to_ascii_lowercase();
        let symbol = [
            ("⌃", Modifiers::CTRL),
            ("⌥", Modifiers::ALT),
            ("⇧", Modifiers::SHIFT),
            ("⌘", Modifiers::COMMAND),
        ]
        .into_iter()
        .find(|(sym, _)| rest.starts_with(sym));
        if let Some((sym, m)) = symbol {
            mods |= m;
            rest = &rest[sym.len()..];
            continue;
        }
        // Words need a separator after them so "c" stays a key.
        let word = [
            ("control", Modifiers::CTRL),
            ("ctrl", Modifiers::CTRL),
            ("option", Modifiers::ALT),
            ("opt", Modifiers::ALT),
            ("alt", Modifiers::ALT),
            ("shift", Modifiers::SHIFT),
            ("command", Modifiers::COMMAND),
            ("cmd", Modifiers::COMMAND),
        ]
        .into_iter()
        .find(|(w, _)| {
            lower.starts_with(w)
                && lower[w.len()..].starts_with(|c: char| c == '+' || c.is_whitespace())
        });
        if let Some((w, m)) = word {
            mods |= m;
            rest = &rest[w.len()..];
            continue;
        }
        break;
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return Err(format!("shortcut “{s}” has no key"));
    }
    match key_from(rest) {
        Some(key) => Ok(Shortcut {
            modifiers: mods,
            key,
        }),
        None => Err(format!("unknown key “{rest}” in shortcut “{s}”")),
    }
}

impl Shortcut {
    /// True if the shortcut uses ⌘ or ⌃, i.e. it can't collide with typing.
    pub fn is_chord(&self) -> bool {
        self.modifiers.command || self.modifiers.ctrl || self.modifiers.mac_cmd
    }

    /// Consume a matching key press (exact modifiers). The key is compared
    /// both as the logical key and as the physical key, so e.g. "⌃`" also
    /// works on layouts where that key position types another character.
    pub fn consume(&self, input: &mut InputState) -> bool {
        let mut hit = false;
        input.events.retain(|e| {
            let is_match = matches!(
                e,
                Event::Key { key, physical_key, pressed: true, modifiers, .. }
                    if (*key == self.key || *physical_key == Some(self.key))
                        && modifiers.matches_exact(self.modifiers)
            );
            hit |= is_match;
            !is_match
        });
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> (Modifiers, Key) {
        let sc = parse(s).unwrap_or_else(|e| panic!("{e}"));
        (sc.modifiers, sc.key)
    }

    #[test]
    fn symbols() {
        assert_eq!(p("⌃`"), (Modifiers::CTRL, Key::Backtick));
        assert_eq!(p("⇧⌘G"), (Modifiers::SHIFT | Modifiers::COMMAND, Key::G));
        assert_eq!(p("⌥⌘T"), (Modifiers::ALT | Modifiers::COMMAND, Key::T));
        assert_eq!(
            p("⌃⌥⇧⌘ + k"),
            (
                Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT | Modifiers::COMMAND,
                Key::K
            )
        );
        assert_eq!(p("⌘↑"), (Modifiers::COMMAND, Key::ArrowUp));
        assert_eq!(p("⌘,"), (Modifiers::COMMAND, Key::Comma));
        assert_eq!(p("F5"), (Modifiers::NONE, Key::F5));
        assert_eq!(p("⌘ + f12"), (Modifiers::COMMAND, Key::F12));
    }

    #[test]
    fn words() {
        assert_eq!(p("ctrl+`"), (Modifiers::CTRL, Key::Backtick));
        assert_eq!(
            p("Cmd+Shift+P"),
            (Modifiers::COMMAND | Modifiers::SHIFT, Key::P)
        );
        assert_eq!(p("alt space"), (Modifiers::ALT, Key::Space));
        assert_eq!(p("c"), (Modifiers::NONE, Key::C));
        assert_eq!(p("ctrl+Enter"), (Modifiers::CTRL, Key::Enter));
    }

    #[test]
    fn errors() {
        assert!(parse("").is_err());
        assert!(parse("⌘").unwrap_err().contains("no key"));
        assert!(parse("⌘Banana").unwrap_err().contains("unknown key"));
    }

    #[test]
    fn chord_detection() {
        assert!(parse("⌃`").unwrap().is_chord());
        assert!(parse("⌘K").unwrap().is_chord());
        assert!(!parse("⌥K").unwrap().is_chord());
        assert!(!parse("F5").unwrap().is_chord());
    }
}
