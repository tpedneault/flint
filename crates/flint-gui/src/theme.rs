//! The colours Flint draws with.
//!
//! Fenix's palettes live in `fenix-gui`, built from `glyphon` types, so
//! Flint can't depend on them; it copies the handful of colours it needs.
//! M1 carries one palette, Orbit Dark (Fenix's default), copied from
//! `crates/fenix-gui/src/theme.rs` at Fenix
//! `f05780740b7d45e41eedc35b846dba7c832f4d49`. M3 adds the other seven and
//! follows Fenix's `editor.theme`; F9 in FENIX-INTEGRATION.md would replace
//! the copy with a shared `fenix-theme` crate.
//!
//! Colours are plain `0..1` RGBA written straight to a non-sRGB surface,
//! exactly as Fenix's are, so the same hex value looks the same in both.

/// What each part of the window is painted with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Fenix's name for the theme.
    pub name: &'static str,
    /// The window background (Fenix's `bg`).
    pub background: [f32; 4],
    /// The query line and footer (Fenix's `bg_modeline`).
    pub surface: [f32; 4],
    /// Body text (Fenix's `fg`).
    pub foreground: [f32; 4],
    /// Subtitles, the placeholder and the footer (Fenix's `gutter_fg`).
    pub muted: [f32; 4],
    /// The caret and highlights (Fenix's `caret`).
    pub accent: [f32; 4],
    /// The selected row (Fenix's `selection`).
    pub selection: [f32; 4],
    /// The window's edge (Fenix's `hl_line`).
    pub border: [f32; 4],
    /// Errors in the footer (Fenix's `git_conflicted` red).
    pub error: [f32; 4],
}

const fn rgba(hex: u32) -> [f32; 4] {
    rgba_alpha(hex, 1.0)
}

const fn rgba_alpha(hex: u32, alpha: f32) -> [f32; 4] {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    [r, g, b, alpha]
}

/// Orbit Dark, Fenix's default theme.
pub const ORBIT_DARK: Palette = Palette {
    name: "Orbit Dark",
    background: rgba(0x1a1b26),
    surface: rgba(0x24283b),
    foreground: rgba(0xc0caf5),
    muted: rgba(0x929bb9),
    accent: rgba(0xe0af68),
    selection: rgba_alpha(0x7aa2f7, 0.12),
    border: rgba(0x222534),
    error: rgba(0xf7768e),
};

/// Converts a palette colour for glyphon.
pub fn text_color(c: [f32; 4]) -> glyphon::Color {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    glyphon::Color::rgba(byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_survives_the_round_trip_to_glyphon() {
        let c = text_color(ORBIT_DARK.foreground);
        assert_eq!((c.r(), c.g(), c.b(), c.a()), (0xc0, 0xca, 0xf5, 0xff));
    }
}
