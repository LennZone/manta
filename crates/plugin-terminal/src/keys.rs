//! egui key events → bytes for the PTY (xterm conventions).

use egui::{Key, Modifiers};

/// Terminal modes that change what keys send.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModes {
    /// DECCKM: arrows/Home/End send `ESC O x` instead of `ESC [ x`.
    pub app_cursor: bool,
    /// Treat Option as Meta (send `ESC` + key) instead of typing the
    /// layout's special character.
    pub option_as_meta: bool,
}

/// xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
fn mod_param(m: Modifiers) -> u8 {
    1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl)
}

/// The ASCII character a key produces without modifiers (US layout), used
/// for Ctrl/Meta combinations where egui gives us no text.
pub fn key_char(key: Key, shift: bool) -> Option<char> {
    let c = match key {
        Key::A => 'a',
        Key::B => 'b',
        Key::C => 'c',
        Key::D => 'd',
        Key::E => 'e',
        Key::F => 'f',
        Key::G => 'g',
        Key::H => 'h',
        Key::I => 'i',
        Key::J => 'j',
        Key::K => 'k',
        Key::L => 'l',
        Key::M => 'm',
        Key::N => 'n',
        Key::O => 'o',
        Key::P => 'p',
        Key::Q => 'q',
        Key::R => 'r',
        Key::S => 's',
        Key::T => 't',
        Key::U => 'u',
        Key::V => 'v',
        Key::W => 'w',
        Key::X => 'x',
        Key::Y => 'y',
        Key::Z => 'z',
        Key::Num0 => '0',
        Key::Num1 => '1',
        Key::Num2 => '2',
        Key::Num3 => '3',
        Key::Num4 => '4',
        Key::Num5 => '5',
        Key::Num6 => '6',
        Key::Num7 => '7',
        Key::Num8 => '8',
        Key::Num9 => '9',
        Key::Minus => '-',
        Key::Equals => '=',
        Key::Plus => '+',
        Key::OpenBracket => '[',
        Key::CloseBracket => ']',
        Key::OpenCurlyBracket => '{',
        Key::CloseCurlyBracket => '}',
        Key::Backslash => '\\',
        Key::Pipe => '|',
        Key::Semicolon => ';',
        Key::Colon => ':',
        Key::Quote => '\'',
        Key::Comma => ',',
        Key::Period => '.',
        Key::Slash => '/',
        Key::Questionmark => '?',
        Key::Exclamationmark => '!',
        Key::Backtick => '`',
        Key::Space => ' ',
        _ => return None,
    };
    Some(if shift { c.to_ascii_uppercase() } else { c })
}

/// Control byte for Ctrl+`c`, if there is one.
fn ctrl_byte(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        ' ' | '2' | '@' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '6' | '^' => 0x1e,
        '-' | '_' | '/' | '7' => 0x1f,
        '8' => 0x7f,
        _ => return None,
    })
}

/// CSI/SS3 sequence for a cursor-type key (`final` is A/B/C/D/H/F).
fn cursor_key(fin: char, m: Modifiers, modes: KeyModes) -> Vec<u8> {
    let p = mod_param(m);
    if p > 1 {
        format!("\x1b[1;{p}{fin}").into_bytes()
    } else if modes.app_cursor {
        format!("\x1bO{fin}").into_bytes()
    } else {
        format!("\x1b[{fin}").into_bytes()
    }
}

/// `CSI n ~` keys (Insert, Delete, PgUp, PgDn, F5–F12).
fn tilde_key(n: u8, m: Modifiers) -> Vec<u8> {
    let p = mod_param(m);
    if p > 1 {
        format!("\x1b[{n};{p}~").into_bytes()
    } else {
        format!("\x1b[{n}~").into_bytes()
    }
}

/// Bytes for a pressed key, or `None` if the key should be handled as text
/// (via egui's `Event::Text`) or not at all.
///
/// macOS conventions on top of xterm: ⌘← / ⌘→ go to line start/end, ⌥← / ⌥→
/// move by word, ⌘⌫ deletes the line, ⌥⌫ deletes a word.
pub fn key_to_bytes(key: Key, m: Modifiers, modes: KeyModes) -> Option<Vec<u8>> {
    // ⌘ combinations are app shortcuts except the few line-editing ones.
    if m.mac_cmd {
        return match key {
            Key::ArrowLeft => Some(vec![0x01]),
            Key::ArrowRight => Some(vec![0x05]),
            Key::Backspace => Some(vec![0x15]),
            _ => None,
        };
    }
    let plain = !m.shift && !m.alt && !m.ctrl;
    let bytes = match key {
        Key::Enter => {
            if m.alt {
                b"\x1b\r".to_vec()
            } else {
                b"\r".to_vec()
            }
        }
        Key::Tab => {
            if m.shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        Key::Backspace => {
            if m.ctrl {
                vec![0x08]
            } else if m.alt {
                b"\x1b\x7f".to_vec()
            } else {
                vec![0x7f]
            }
        }
        Key::Escape => vec![0x1b],
        Key::ArrowUp => cursor_key('A', m, modes),
        Key::ArrowDown => cursor_key('B', m, modes),
        Key::ArrowRight if m.alt && !m.ctrl && !m.shift => b"\x1bf".to_vec(),
        Key::ArrowLeft if m.alt && !m.ctrl && !m.shift => b"\x1bb".to_vec(),
        Key::ArrowRight => cursor_key('C', m, modes),
        Key::ArrowLeft => cursor_key('D', m, modes),
        Key::Home => cursor_key('H', m, modes),
        Key::End => cursor_key('F', m, modes),
        Key::Insert => tilde_key(2, m),
        Key::Delete => tilde_key(3, m),
        Key::PageUp => tilde_key(5, m),
        Key::PageDown => tilde_key(6, m),
        Key::F1 | Key::F2 | Key::F3 | Key::F4 => {
            let fin = match key {
                Key::F1 => 'P',
                Key::F2 => 'Q',
                Key::F3 => 'R',
                _ => 'S',
            };
            let p = mod_param(m);
            if p > 1 {
                format!("\x1b[1;{p}{fin}").into_bytes()
            } else {
                format!("\x1bO{fin}").into_bytes()
            }
        }
        Key::F5 => tilde_key(15, m),
        Key::F6 => tilde_key(17, m),
        Key::F7 => tilde_key(18, m),
        Key::F8 => tilde_key(19, m),
        Key::F9 => tilde_key(20, m),
        Key::F10 => tilde_key(21, m),
        Key::F11 => tilde_key(23, m),
        Key::F12 => tilde_key(24, m),
        _ => {
            if plain {
                return None;
            }
            if m.ctrl {
                let c = key_char(key, false)?;
                let b = ctrl_byte(c)?;
                return Some(if m.alt { vec![0x1b, b] } else { vec![b] });
            }
            if m.alt && modes.option_as_meta {
                let c = key_char(key, m.shift)?;
                let mut v = vec![0x1b];
                let mut buf = [0u8; 4];
                v.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                return Some(v);
            }
            return None;
        }
    };
    Some(bytes)
}

/// Text typed by the user (from `Event::Text`).
pub fn text_to_bytes(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// Paste payload: newlines become CR (what Enter sends); with bracketed
/// paste the text is wrapped and any embedded end marker is defused.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let safe = normalized.replace('\x1b', "");
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(safe.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        normalized.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;
    fn m(shift: bool, alt: bool, ctrl: bool) -> Modifiers {
        Modifiers {
            alt,
            ctrl,
            shift,
            mac_cmd: false,
            command: false,
        }
    }
    fn k(key: Key, mods: Modifiers) -> Option<Vec<u8>> {
        key_to_bytes(key, mods, KeyModes::default())
    }

    #[test]
    fn basics() {
        assert_eq!(k(Key::Enter, NONE).unwrap(), b"\r");
        assert_eq!(k(Key::Backspace, NONE).unwrap(), b"\x7f");
        assert_eq!(k(Key::Tab, NONE).unwrap(), b"\t");
        assert_eq!(k(Key::Tab, m(true, false, false)).unwrap(), b"\x1b[Z");
        assert_eq!(k(Key::Escape, NONE).unwrap(), b"\x1b");
        assert_eq!(k(Key::Delete, NONE).unwrap(), b"\x1b[3~");
        assert_eq!(k(Key::PageUp, NONE).unwrap(), b"\x1b[5~");
        assert_eq!(k(Key::PageDown, NONE).unwrap(), b"\x1b[6~");
        // Printable keys come through Event::Text.
        assert_eq!(k(Key::A, NONE), None);
        assert_eq!(k(Key::A, m(true, false, false)), None);
    }

    #[test]
    fn arrows_and_app_cursor() {
        assert_eq!(k(Key::ArrowUp, NONE).unwrap(), b"\x1b[A");
        assert_eq!(k(Key::ArrowLeft, NONE).unwrap(), b"\x1b[D");
        assert_eq!(k(Key::Home, NONE).unwrap(), b"\x1b[H");
        assert_eq!(k(Key::End, NONE).unwrap(), b"\x1b[F");
        let app = KeyModes {
            app_cursor: true,
            option_as_meta: false,
        };
        assert_eq!(key_to_bytes(Key::ArrowUp, NONE, app).unwrap(), b"\x1bOA");
        assert_eq!(key_to_bytes(Key::ArrowRight, NONE, app).unwrap(), b"\x1bOC");
        assert_eq!(key_to_bytes(Key::Home, NONE, app).unwrap(), b"\x1bOH");
        // Modified arrows always use CSI 1;m.
        assert_eq!(
            key_to_bytes(Key::ArrowUp, m(true, false, false), app).unwrap(),
            b"\x1b[1;2A"
        );
        assert_eq!(
            k(Key::ArrowRight, m(false, false, true)).unwrap(),
            b"\x1b[1;5C"
        );
        // macOS word / line motions.
        assert_eq!(k(Key::ArrowLeft, m(false, true, false)).unwrap(), b"\x1bb");
        assert_eq!(k(Key::ArrowRight, m(false, true, false)).unwrap(), b"\x1bf");
        let cmd = Modifiers {
            mac_cmd: true,
            command: true,
            ..NONE
        };
        assert_eq!(k(Key::ArrowLeft, cmd).unwrap(), b"\x01");
        assert_eq!(k(Key::ArrowRight, cmd).unwrap(), b"\x05");
        assert_eq!(k(Key::Backspace, cmd).unwrap(), b"\x15");
        assert_eq!(k(Key::C, cmd), None);
    }

    #[test]
    fn function_keys() {
        assert_eq!(k(Key::F1, NONE).unwrap(), b"\x1bOP");
        assert_eq!(k(Key::F4, NONE).unwrap(), b"\x1bOS");
        assert_eq!(k(Key::F5, NONE).unwrap(), b"\x1b[15~");
        assert_eq!(k(Key::F10, NONE).unwrap(), b"\x1b[21~");
        assert_eq!(k(Key::F12, NONE).unwrap(), b"\x1b[24~");
        assert_eq!(k(Key::F1, m(true, false, false)).unwrap(), b"\x1b[1;2P");
        assert_eq!(k(Key::F5, m(false, false, true)).unwrap(), b"\x1b[15;5~");
    }

    #[test]
    fn ctrl_keys() {
        let c = m(false, false, true);
        assert_eq!(k(Key::C, c).unwrap(), vec![0x03]);
        assert_eq!(k(Key::A, c).unwrap(), vec![0x01]);
        assert_eq!(k(Key::Z, c).unwrap(), vec![0x1a]);
        assert_eq!(k(Key::D, c).unwrap(), vec![0x04]);
        assert_eq!(k(Key::OpenBracket, c).unwrap(), vec![0x1b]);
        assert_eq!(k(Key::Space, c).unwrap(), vec![0x00]);
        assert_eq!(k(Key::Backslash, c).unwrap(), vec![0x1c]);
        assert_eq!(k(Key::Backspace, c).unwrap(), vec![0x08]);
        // Ctrl+Shift+letter is the same control byte.
        assert_eq!(k(Key::C, m(true, false, true)).unwrap(), vec![0x03]);
        // Ctrl+Alt+letter: ESC prefix.
        assert_eq!(k(Key::X, m(false, true, true)).unwrap(), vec![0x1b, 0x18]);
    }

    #[test]
    fn option_as_meta() {
        let alt = m(false, true, false);
        // Off: Option types the layout's character (via Event::Text).
        assert_eq!(k(Key::B, alt), None);
        let meta = KeyModes {
            app_cursor: false,
            option_as_meta: true,
        };
        assert_eq!(key_to_bytes(Key::B, alt, meta).unwrap(), b"\x1bb");
        assert_eq!(
            key_to_bytes(Key::B, m(true, true, false), meta).unwrap(),
            b"\x1bB"
        );
        assert_eq!(key_to_bytes(Key::Period, alt, meta).unwrap(), b"\x1b.");
        assert_eq!(k(Key::Backspace, alt).unwrap(), b"\x1b\x7f");
        assert_eq!(k(Key::Enter, alt).unwrap(), b"\x1b\r");
    }

    #[test]
    fn text_and_paste() {
        assert_eq!(text_to_bytes("ä€🚀"), "ä€🚀".as_bytes());
        assert_eq!(paste_bytes("a\nb\r\nc", false), b"a\rb\rc");
        assert_eq!(paste_bytes("x\ny", true), b"\x1b[200~x\ry\x1b[201~");
        assert_eq!(
            paste_bytes("evil\x1b[201~rm", true),
            b"\x1b[200~evil[201~rm\x1b[201~"
        );
    }
}
