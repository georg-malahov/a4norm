//! The answers' font: Arimo Regular (fonts/, SIL Open Font License 1.1),
//! metric-compatible with Arial and Helvetica, with Latin Extended, Cyrillic
//! and Greek: "Yılmaz", "Şahin", "Łukasz", "Đorđević", "Ștefan" as written.
//! It is in the module, so nothing is fetched; a PDF embeds the glyphs it
//! uses (pdf.rs).

use std::sync::OnceLock;
use ttf_parser::{Face, GlyphId};

pub static DATA: &[u8] = include_bytes!("../fonts/Arimo-Regular.ttf");

/// Cap height and descent, per em (Arimo's OS/2 cap height 1409 and hhea
/// descent 434 in 2048 units).
pub const CAP: f32 = 1409.0 / 2048.0;
pub const DESCENT: f32 = 434.0 / 2048.0;

pub fn face() -> &'static Face<'static> {
    static FACE: OnceLock<Face<'static>> = OnceLock::new();
    FACE.get_or_init(|| Face::parse(DATA, 0).expect("Arimo"))
}

/// The glyph a character is set with: its own, or "?" when the font has
/// none (the second value says so).
pub fn glyph(c: char) -> (GlyphId, bool) {
    let f = face();
    match f.glyph_index(c) {
        Some(g) => (g, false),
        None => (f.glyph_index('?').unwrap_or(GlyphId(0)), true),
    }
}

/// Whether some character of `s` is not in the font (and is set as "?").
pub fn lost(s: &str) -> bool {
    s.chars().any(|c| glyph(c).1)
}

/// A glyph's advance in 1/1000 em.
pub fn advance(g: GlyphId) -> f32 {
    let f = face();
    f.glyph_hor_advance(g).unwrap_or(0) as f32 * 1000.0 / f.units_per_em() as f32
}

/// The width of `s` set at `size`, in the same unit as `size`.
pub fn width(s: &str, size: f32) -> f32 {
    s.chars().map(|c| advance(glyph(c).0)).sum::<f32>() * size / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_of_our_users() {
        for s in ["Yılmaz", "Şahin", "Łukasz", "Đorđević", "Ștefan", "Müller-Straße 5", "ß", "Иванов", "Ελένη", "€"] {
            assert!(!lost(s), "{s}");
        }
        assert!(lost("日本"));
        let f = face();
        assert_eq!(f.capital_height(), Some(1409));
        assert_eq!(f.descender(), -434);
        // Arimo is Arial's metrics: "Hello" is H e l l o = 722 556 222 222 556
        assert!((width("Hello", 10.0) - 22.78).abs() < 0.02, "{}", width("Hello", 10.0));
    }
}
