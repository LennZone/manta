//! Quoting paths for the shell command line we type on the user's behalf.

/// Which shell family we are typing into (affects quoting and the line
/// editor tricks used to keep the user's half-typed input).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Zsh,
    Bash,
    Fish,
    /// Any other POSIX-ish shell (sh, dash, ksh, …).
    Other,
}

impl ShellKind {
    pub fn from_path(path: &std::path::Path) -> ShellKind {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let name = name.trim_start_matches('-');
        match name {
            n if n == "zsh" || n.starts_with("zsh-") => ShellKind::Zsh,
            n if n == "bash" || n.starts_with("bash-") => ShellKind::Bash,
            "fish" => ShellKind::Fish,
            _ => ShellKind::Other,
        }
    }
}

/// Quote `s` as a single shell word.
///
/// POSIX shells (zsh, bash, sh): single quotes with `'` written as `'\''`.
/// If the string contains control characters (a newline in a path would
/// submit the line early), ANSI-C quoting `$'…'` is used instead (zsh, bash
/// and ksh support it). Fish uses its own single-quote rules (`\'`, `\\`).
pub fn quote(s: &str, kind: ShellKind) -> String {
    let has_ctrl = s.chars().any(|c| c.is_control());
    match kind {
        ShellKind::Fish => {
            if has_ctrl {
                // Fish: outside quotes, `\n`, `\t`, `\xHH` escapes work.
                let mut out = String::new();
                for c in s.chars() {
                    match c {
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
                        c if c.is_alphanumeric() || "/._-+,@%".contains(c) => out.push(c),
                        c => {
                            out.push('\\');
                            out.push(c);
                        }
                    }
                }
                out
            } else {
                format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
            }
        }
        _ => {
            if has_ctrl {
                let mut out = String::from("$'");
                for c in s.chars() {
                    match c {
                        '\\' => out.push_str("\\\\"),
                        '\'' => out.push_str("\\'"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        '\r' => out.push_str("\\r"),
                        c if c.is_control() => {
                            // Control chars are < 0x80 or C1 (0x80–0x9f); encode
                            // as UTF-8 bytes.
                            let mut buf = [0u8; 4];
                            for b in c.encode_utf8(&mut buf).bytes() {
                                out.push_str(&format!("\\x{b:02x}"));
                            }
                        }
                        c => out.push(c),
                    }
                }
                out.push('\'');
                out
            } else {
                format!("'{}'", s.replace('\'', "'\\''"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_quoting() {
        let z = ShellKind::Zsh;
        assert_eq!(quote("/tmp/a", z), "'/tmp/a'");
        assert_eq!(quote("/tmp/with space", z), "'/tmp/with space'");
        assert_eq!(quote("/tmp/it's", z), "'/tmp/it'\\''s'");
        assert_eq!(quote("/tmp/''", z), "'/tmp/'\\'''\\'''");
        assert_eq!(
            quote("/Users/anna/Übergröße 🚀", z),
            "'/Users/anna/Übergröße 🚀'"
        );
        assert_eq!(
            quote("/tmp/$HOME `x` \"q\" \\", ShellKind::Bash),
            "'/tmp/$HOME `x` \"q\" \\'"
        );
        assert_eq!(quote("/tmp/a*b?[c]", z), "'/tmp/a*b?[c]'");
    }

    #[test]
    fn control_chars_use_ansi_c() {
        assert_eq!(quote("/tmp/a\nb", ShellKind::Zsh), "$'/tmp/a\\nb'");
        assert_eq!(quote("/tmp/it's\n", ShellKind::Bash), "$'/tmp/it\\'s\\n'");
        assert_eq!(quote("/a\u{1b}b", ShellKind::Zsh), "$'/a\\x1bb'");
    }

    #[test]
    fn fish_quoting() {
        assert_eq!(quote("/tmp/it's", ShellKind::Fish), "'/tmp/it\\'s'");
        assert_eq!(quote("/tmp/a\\b", ShellKind::Fish), "'/tmp/a\\\\b'");
        assert_eq!(quote("/tmp/a b\nc", ShellKind::Fish), "/tmp/a\\ b\\nc");
    }

    #[test]
    fn kind_detection() {
        use std::path::Path;
        assert_eq!(ShellKind::from_path(Path::new("/bin/zsh")), ShellKind::Zsh);
        assert_eq!(
            ShellKind::from_path(Path::new("/opt/homebrew/bin/bash")),
            ShellKind::Bash
        );
        assert_eq!(ShellKind::from_path(Path::new("-zsh")), ShellKind::Zsh);
        assert_eq!(
            ShellKind::from_path(Path::new("/usr/local/bin/fish")),
            ShellKind::Fish
        );
        assert_eq!(ShellKind::from_path(Path::new("/bin/sh")), ShellKind::Other);
    }
}
