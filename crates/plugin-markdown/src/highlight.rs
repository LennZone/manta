//! Syntax highlighting of fenced code with syntect (pure-Rust regex
//! engine). Both theme variants are computed once, off the UI thread, when
//! the document is parsed; the renderer only picks colours.

use std::sync::OnceLock;

use egui::Color32;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// Code longer than this is shown without colours.
const MAX_HIGHLIGHT_BYTES: usize = 200_000;

/// One coloured run: byte range into the code text.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub color: Color32,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Highlight {
    pub light: Vec<Span>,
    pub dark: Vec<Span>,
}

struct Assets {
    syntaxes: SyntaxSet,
    light: Theme,
    dark: Theme,
}

fn assets() -> &'static Assets {
    static A: OnceLock<Assets> = OnceLock::new();
    A.get_or_init(|| {
        let mut themes = ThemeSet::load_defaults().themes;
        Assets {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            light: themes.remove("InspiredGitHub").unwrap_or_default(),
            dark: themes.remove("base16-ocean.dark").unwrap_or_default(),
        }
    })
}

/// Load grammars and themes now (e.g. on the loader thread at startup).
pub fn warm_up() {
    let _ = assets();
}

fn syntax_for<'a>(set: &'a SyntaxSet, lang: &str) -> Option<&'a SyntaxReference> {
    let lang = lang.trim().to_ascii_lowercase();
    if lang.is_empty() {
        return None;
    }
    let token = match lang.as_str() {
        "ts" | "typescript" | "tsx" | "jsx" | "mjs" | "cjs" | "javascript" | "node" => "js",
        "sh" | "zsh" | "shell" | "console" | "shellsession" | "bash" => "bash",
        "yml" => "yaml",
        "py" | "python3" => "python",
        "rs" => "rust",
        "md" | "mdx" => "markdown",
        "c++" | "cxx" => "cpp",
        "objc" | "objective-c" => "m",
        "golang" => "go",
        "html5" | "vue" | "svelte" | "xml" | "svg" => "html",
        "json5" | "jsonc" => "json",
        other => other,
    };
    set.find_syntax_by_token(token)
}

fn spans(syntax: &SyntaxReference, theme: &Theme, code: &str) -> Option<Vec<Span>> {
    let a = assets();
    let mut h = HighlightLines::new(syntax, theme);
    let mut out = Vec::new();
    let mut at = 0;
    for line in LinesWithEndings::from(code) {
        let ranges = h.highlight_line(line, &a.syntaxes).ok()?;
        for (style, text) in ranges {
            let c = style.foreground;
            let span = Span {
                start: at,
                end: at + text.len(),
                color: Color32::from_rgb(c.r, c.g, c.b),
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
            };
            at += text.len();
            // Merge runs of the same look (fewer layout sections).
            if let Some(last) = out.last_mut() {
                let l: &mut Span = last;
                if l.color == span.color
                    && l.bold == span.bold
                    && l.italic == span.italic
                    && l.end == span.start
                {
                    l.end = span.end;
                    continue;
                }
            }
            out.push(span);
        }
    }
    Some(out)
}

/// Colours for `code` in `lang` (fence info), or `None` for an unknown or
/// missing language or very long code.
pub fn highlight(lang: &str, code: &str) -> Option<Highlight> {
    if code.len() > MAX_HIGHLIGHT_BYTES {
        return None;
    }
    let a = assets();
    let syntax = syntax_for(&a.syntaxes, lang)?;
    Some(Highlight {
        light: spans(syntax, &a.light, code)?,
        dark: spans(syntax, &a.dark, code)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_gets_colours_in_both_themes() {
        let code = "fn main() {\n    let x = \"hi\"; // greet\n}";
        let h = highlight("rust", code).expect("rust is known");
        for spans in [&h.light, &h.dark] {
            assert_eq!(spans.first().unwrap().start, 0);
            assert_eq!(spans.last().unwrap().end, code.len());
            let colours: std::collections::HashSet<_> = spans.iter().map(|s| s.color).collect();
            assert!(colours.len() >= 3, "{colours:?}");
        }
        assert_ne!(h.light, h.dark);
    }

    #[test]
    fn aliases_and_unknown_languages() {
        assert!(highlight("ts", "const a: number = 1;").is_some());
        assert!(highlight("sh", "echo hi").is_some());
        assert!(highlight("klingon", "qapla'").is_none());
        assert!(highlight("", "plain").is_none());
    }
}
