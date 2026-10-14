//! The answers' font: Arimo Regular, and Arimo Bold for a field styled bold
//! (fonts/, SIL Open Font License 1.1), metric-compatible with Arial and
//! Helvetica, with Latin Extended, Cyrillic and Greek: "Yılmaz", "Şahin",
//! "Łukasz", "Đorđević", "Ștefan" as written. Both are in the module, so
//! nothing is fetched; a PDF embeds the glyphs it uses of each (pdf.rs).

use std::sync::OnceLock;
use ttf_parser::{Face, GlyphId};

pub static DATA: &[u8] = include_bytes!("../fonts/Arimo-Regular.ttf");
pub static BOLD_DATA: &[u8] = include_bytes!("../fonts/Arimo-Bold.ttf");

/// Cap height and descent, per em (Arimo's OS/2 cap height 1409 and hhea
/// descent 434 in 2048 units, the same in its Bold).
pub const CAP: f32 = 1409.0 / 2048.0;
pub const DESCENT: f32 = 434.0 / 2048.0;

/// Arimo Regular.
pub fn face() -> &'static Face<'static> {
    static FACE: OnceLock<Face<'static>> = OnceLock::new();
    FACE.get_or_init(|| Face::parse(DATA, 0).expect("Arimo"))
}

/// Arimo Regular, or its Bold.
pub fn face_of(bold: bool) -> &'static Face<'static> {
    static BOLD: OnceLock<Face<'static>> = OnceLock::new();
    if bold {
        BOLD.get_or_init(|| Face::parse(BOLD_DATA, 0).expect("Arimo Bold"))
    } else {
        face()
    }
}

/// The glyph a character is set with: its own, or "?" when the font has
/// none (the second value says so).
pub fn glyph(c: char) -> (GlyphId, bool) {
    glyph_of(false, c)
}

/// As `glyph`, in the Regular or the Bold.
pub fn glyph_of(bold: bool, c: char) -> (GlyphId, bool) {
    let f = face_of(bold);
    match f.glyph_index(c) {
        Some(g) => (g, false),
        None => (f.glyph_index('?').unwrap_or(GlyphId(0)), true),
    }
}

/// Whether some character of `s` is not in the font (and is set as "?").
pub fn lost(s: &str) -> bool {
    lost_of(false, s)
}

/// As `lost`, in the Regular or the Bold.
pub fn lost_of(bold: bool, s: &str) -> bool {
    s.chars().any(|c| glyph_of(bold, c).1)
}

/// A glyph's advance in 1/1000 em.
pub fn advance(g: GlyphId) -> f32 {
    advance_of(false, g)
}

/// As `advance`, in the Regular or the Bold.
pub fn advance_of(bold: bool, g: GlyphId) -> f32 {
    let f = face_of(bold);
    f.glyph_hor_advance(g).unwrap_or(0) as f32 * 1000.0 / f.units_per_em() as f32
}

/// The width of `s` set at `size`, in the same unit as `size`.
pub fn width(s: &str, size: f32) -> f32 {
    width_of(false, s, size)
}

/// As `width`, in the Regular or the Bold.
pub fn width_of(bold: bool, s: &str, size: f32) -> f32 {
    s.chars().map(|c| advance_of(bold, glyph_of(bold, c).0)).sum::<f32>() * size / 1000.0
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

    #[test]
    fn the_names_in_bold() {
        for s in ["Yılmaz", "Şahin", "Łukasz", "Đorđević", "Ștefan", "Müller-Straße 5", "NEIN – nicht einverstanden", "Иванов", "Ελένη", "€"] {
            assert!(!lost_of(true, s), "{s}");
        }
        let f = face_of(true);
        assert_eq!(f.capital_height(), Some(1409));
        assert_eq!(f.descender(), -434);
        // Arial Bold's metrics: "Hello" is H e l l o = 722 556 278 278 611
        assert!((width_of(true, "Hello", 10.0) - 24.45).abs() < 0.02, "{}", width_of(true, "Hello", 10.0));
        assert!(width_of(true, "Hello", 10.0) > width("Hello", 10.0));
    }
}
