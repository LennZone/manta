//! Terminal colours for light and dark mode.
//!
//! The default background/foreground match the app's content surfaces
//! (`extreme_bg_color` and the text colour of `crates/app/src/theme.rs`). The
//! 16 ANSI colours are tuned so every one is readable on that background:
//! "black" stays visible in dark mode and "white" stays visible in light mode.

use egui::Color32;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const DARK_ANSI: [Color32; 16] = [
    rgb(84, 84, 92),    // black
    rgb(255, 107, 98),  // red
    rgb(102, 204, 122), // green
    rgb(232, 192, 92),  // yellow
    rgb(96, 160, 255),  // blue
    rgb(206, 132, 245), // magenta
    rgb(84, 200, 212),  // cyan
    rgb(204, 204, 210), // white
    rgb(132, 132, 140), // bright black
    rgb(255, 142, 132), // bright red
    rgb(136, 226, 154), // bright green
    rgb(250, 214, 122), // bright yellow
    rgb(136, 188, 255), // bright blue
    rgb(224, 164, 255), // bright magenta
    rgb(124, 222, 232), // bright cyan
    rgb(246, 246, 248), // bright white
];

pub const LIGHT_ANSI: [Color32; 16] = [
    rgb(29, 29, 31),    // black
    rgb(196, 30, 42),   // red
    rgb(24, 122, 52),   // green
    rgb(150, 98, 0),    // yellow
    rgb(0, 92, 204),    // blue
    rgb(158, 56, 186),  // magenta
    rgb(0, 124, 138),   // cyan
    rgb(142, 142, 148), // white
    rgb(104, 104, 110), // bright black
    rgb(222, 58, 58),   // bright red
    rgb(36, 148, 72),   // bright green
    rgb(176, 120, 0),   // bright yellow
    rgb(36, 118, 230),  // bright blue
    rgb(180, 80, 210),  // bright magenta
    rgb(0, 150, 164),   // bright cyan
    rgb(120, 120, 126), // bright white
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub fg: Color32,
    pub bg: Color32,
    pub cursor: Color32,
    pub selection: Color32,
    pub ansi: [Color32; 16],
}

impl Theme {
    pub fn dark() -> Theme {
        Theme {
            dark: true,
            fg: rgb(236, 236, 240),
            bg: rgb(28, 28, 30),
            cursor: rgb(64, 156, 255),
            selection: Color32::from_rgba_unmultiplied(64, 140, 255, 90),
            ansi: DARK_ANSI,
        }
    }

    pub fn light() -> Theme {
        Theme {
            dark: false,
            fg: rgb(29, 29, 31),
            bg: rgb(255, 255, 255),
            cursor: rgb(0, 122, 255),
            selection: Color32::from_rgba_unmultiplied(0, 110, 255, 60),
            ansi: LIGHT_ANSI,
        }
    }

    /// Follow the current egui visuals (the app sets them per theme).
    pub fn from_visuals(v: &egui::Visuals) -> Theme {
        let mut t = if v.dark_mode {
            Theme::dark()
        } else {
            Theme::light()
        };
        t.bg = v.extreme_bg_color;
        t
    }

    /// One of the 256 indexed colours.
    pub fn indexed(&self, i: u8) -> Color32 {
        match i {
            0..=15 => self.ansi[i as usize],
            16..=231 => {
                const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
                let i = i - 16;
                rgb(
                    LEVELS[(i / 36) as usize],
                    LEVELS[((i / 6) % 6) as usize],
                    LEVELS[(i % 6) as usize],
                )
            }
            232..=255 => {
                let g = 8 + 10 * (i - 232);
                rgb(g, g, g)
            }
        }
    }
}

/// Linear blend in sRGB space (good enough for dimming).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: Color32) -> f32 {
        let f = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn ansi_readable_on_default_bg() {
        for t in [Theme::light(), Theme::dark()] {
            for (i, c) in t.ansi.iter().enumerate() {
                // "black" on dark / "white" on light are decorative; keep
                // them at least distinguishable.
                let min = if (t.dark && i == 0) || (!t.dark && (i == 7 || i == 15)) {
                    2.0
                } else {
                    3.0
                };
                let ratio = contrast(*c, t.bg);
                assert!(ratio >= min, "dark={} idx={i} ratio={ratio}", t.dark);
            }
            assert!(contrast(t.fg, t.bg) > 12.0);
        }
    }

    #[test]
    fn cube_and_greys() {
        let t = Theme::dark();
        assert_eq!(t.indexed(16), rgb(0, 0, 0));
        assert_eq!(t.indexed(196), rgb(255, 0, 0));
        assert_eq!(t.indexed(231), rgb(255, 255, 255));
        assert_eq!(t.indexed(232), rgb(8, 8, 8));
        assert_eq!(t.indexed(255), rgb(238, 238, 238));
    }
}
