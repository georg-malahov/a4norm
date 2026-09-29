//! Helvetica, one of the PDF's standard fonts: every viewer has it, so
//! nothing is embedded. Text is set in WinAnsi (cp1252), which holds German
//! (ä ö ü ß), the other western European letters, € and the typographic
//! quotes and dashes.

/// Advance widths in 1/1000 em, by WinAnsi byte (Adobe's Helvetica AFM); 0
/// where the encoding has no character.
const WIDTHS: [u16; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, 0,
    556, 0, 222, 556, 333, 1000, 556, 556, 333, 1000, 667, 333, 1000, 0, 611, 0,
    0, 222, 222, 333, 333, 350, 556, 1000, 333, 1000, 500, 333, 944, 0, 500, 667,
    278, 333, 556, 556, 556, 556, 260, 556, 333, 737, 370, 556, 584, 333, 737, 333,
    400, 584, 333, 333, 333, 556, 537, 278, 333, 333, 365, 556, 834, 834, 834, 611,
    667, 667, 667, 667, 667, 667, 1000, 722, 667, 667, 667, 667, 278, 278, 278, 278,
    722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722, 722, 667, 667, 611,
    556, 556, 556, 556, 556, 556, 889, 500, 556, 556, 556, 556, 278, 278, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 584, 611, 556, 556, 556, 556, 500, 556, 500,];

/// The WinAnsi bytes 0x80–0x9F, where cp1252 departs from Latin-1.
const HIGH: [(char, u8); 27] = [
    ('€', 0x80), ('‚', 0x82), ('ƒ', 0x83), ('„', 0x84), ('…', 0x85), ('†', 0x86), ('‡', 0x87),
    ('ˆ', 0x88), ('‰', 0x89), ('Š', 0x8A), ('‹', 0x8B), ('Œ', 0x8C), ('Ž', 0x8E), ('‘', 0x91),
    ('’', 0x92), ('“', 0x93), ('”', 0x94), ('•', 0x95), ('–', 0x96), ('—', 0x97), ('˜', 0x98),
    ('™', 0x99), ('š', 0x9A), ('›', 0x9B), ('œ', 0x9C), ('ž', 0x9E), ('Ÿ', 0x9F),
];

/// Cap height and descent, per em.
pub const CAP: f32 = 0.718;
pub const DESCENT: f32 = 0.207;

/// A character's WinAnsi byte, if Helvetica can set it.
pub fn byte(c: char) -> Option<u8> {
    let b = match c as u32 {
        0x20..=0x7E | 0xA0..=0xFF => c as u32 as u8,
        _ => HIGH.iter().find(|(h, _)| *h == c)?.1,
    };
    (WIDTHS[b as usize] > 0).then_some(b)
}

/// `s` in WinAnsi; a character it lacks becomes "?", and the second value
/// says whether that happened.
pub fn encode(s: &str) -> (Vec<u8>, bool) {
    let mut lost = false;
    let bytes = s
        .chars()
        .map(|c| {
            byte(c).unwrap_or_else(|| {
                lost = true;
                b'?'
            })
        })
        .collect();
    (bytes, lost)
}

/// The width of `s` set at `size`, in the same unit as `size`.
pub fn width(s: &str, size: f32) -> f32 {
    encode(s).0.iter().map(|&b| WIDTHS[b as usize] as f32).sum::<f32>() * size / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german() {
        assert_eq!(encode("Straße Äöü €").0, b"Stra\xdfe \xc4\xf6\xfc \x80");
        assert!(!encode("Hüfte").1 && encode("Łódź").1);
        assert!((width("Hello", 10.0) - 22.78).abs() < 1e-3); // H e l l o: 722 556 222 222 556
    }
}
