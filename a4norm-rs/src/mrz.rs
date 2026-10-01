//! A machine-readable zone (ICAO 9303) found by its geometry alone, no
//! reading: two or three lines of OCR-B at one fixed pitch, 10 characters
//! to 25.4 mm, 44 to the line on a passport (TD3), 36 on a TD2 card or visa,
//! 30 on an ID-1 card (TD1, three lines). Its pitch is a ruler printed on
//! the document: millimetres per pixel without a guess at the format.
//!
//! On the document as rectified (upright, nothing around it): bands of
//! rows with ink; in each, the
//! glyphs as runs of inked columns; a band is a line of the zone when the
//! glyphs' middles fall on one even pitch and span 30, 36 or 44 of it;
//! two (three) such lines of one count, one pitch and one left edge, a
//! line and a bit apart, are the zone.

use crate::ops::Plane;

/// The zone's pitch in mm: 10 characters per inch.
pub const PITCH_MM: f64 = 2.54;

#[derive(Debug, Clone, PartialEq)]
pub struct Mrz {
    /// 44 (TD3), 36 (TD2) or 30 (TD1)
    pub chars: usize,
    pub lines: usize,
    /// pixels per character
    pub pitch: f64,
    /// the zone's box: x0, y0, x1, y1 in pixels
    pub bbox: [usize; 4],
}

impl Mrz {
    /// Millimetres per pixel of the picture it was found in.
    pub fn mm_per_px(&self) -> f64 {
        PITCH_MM / self.pitch
    }
    pub fn kind(&self) -> &'static str {
        match self.chars {
            44 => "TD3",
            36 => "TD2",
            _ => "TD1",
        }
    }
}

/// `g` turned on its side (rows for columns): a zone running down a page
/// lies across this.
pub fn transposed(g: &Plane) -> Plane {
    let (w, h) = (g.w, g.h);
    let mut d = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            d[x * h + y] = g.d[y * w + x];
        }
    }
    Plane { w: h, h: w, d }
}

/// A line of glyphs at one pitch.
#[derive(Debug, Clone, Copy)]
struct Row {
    y0: usize,
    y1: usize,
    x0: f64,
    x1: f64,
    pitch: f64,
    chars: usize,
}

/// The zone in `g` (luma 0..1), if there is one.
pub fn find(g: &Plane) -> Option<Mrz> {
    let (w, h) = (g.w, g.h);
    if w < 200 || h < 100 {
        return None;
    }
    // ink: well under the paper's level (the 90th percentile; a tinted
    // passport page is the paper here)
    let mut s: Vec<f32> = g.d.iter().step_by(7).copied().collect();
    s.sort_by(f32::total_cmp);
    let paper = s[(s.len() - 1) * 9 / 10];
    let thr = 0.5 * paper;
    let dark = |x: usize, y: usize| g.d[y * w + x] < thr;
    // bands of rows with ink, a gap of one row bridged; counted across the
    // whole width and across its right two thirds, where a passport's
    // photo does not run the zone's lines together
    let mut bands: Vec<(usize, usize)> = vec![];
    for x0 in [0, w / 3] {
        let span = w - x0;
        let ink: Vec<usize> = (0..h).map(|y| (x0..w).filter(|&x| dark(x, y)).count()).collect();
        let on = |y: usize| ink[y] * 50 >= span;
        let mut y = 0;
        while y < h {
            if !on(y) {
                y += 1;
                continue;
            }
            let y0 = y;
            while y < h && (on(y) || (y + 1 < h && on(y + 1))) {
                y += 1;
            }
            if y - y0 >= 6 && !bands.contains(&(y0, y)) {
                bands.push((y0, y));
            }
        }
    }
    bands.sort_unstable();
    // a line of the zone runs across most of the document (111.8 of 125 mm
    // on a passport, 76.2 of 85.6 on a card): `g` is the document alone
    let rows: Vec<Row> = bands.iter().filter_map(|&(y0, y1)| row(&dark, w, y0, y1)).filter(|r| r.x1 - r.x0 >= 0.6 * w as f64).collect();
    // two or three lines of one count, pitch and left edge, a line apart
    let mut best: Option<Mrz> = None;
    for (i, a) in rows.iter().enumerate() {
        let want = if a.chars == 30 { 3 } else { 2 };
        let mut group = vec![*a];
        for b in &rows[i + 1..] {
            let last = group.last().unwrap();
            let gap = ((b.y0 + b.y1) as f64 - (last.y0 + last.y1) as f64) / 2.0;
            let same = b.chars == a.chars && (b.pitch / a.pitch - 1.0).abs() < 0.08 && (b.x0 - a.x0).abs() < 1.5 * a.pitch;
            if same && gap > 1.1 * a.pitch && gap < 3.2 * a.pitch {
                group.push(*b);
                if group.len() == want {
                    break;
                }
            } else if gap >= 3.2 * a.pitch {
                break;
            }
        }
        let pitch = group.iter().map(|r| r.pitch).sum::<f64>() / group.len() as f64;
        let x0 = group.iter().map(|r| r.x0).fold(f64::MAX, f64::min);
        let x1 = group.iter().map(|r| r.x1).fold(f64::MIN, f64::max);
        let bbox = [x0.max(0.0) as usize, group[0].y0, (x1 as usize).min(w), group.last().unwrap().y1];
        let m = Mrz { chars: a.chars, lines: group.len(), pitch, bbox };
        if group.len() == want {
            return Some(m);
        }
        // one line of a passport's or a TD2's alone (the other cut off by
        // the frame, or lost in glare): kept, if nothing better comes
        if a.chars != 30 && best.is_none() {
            best = Some(m);
        }
    }
    best
}

/// The band `y0..y1` as a line of the zone: its glyphs (runs of inked
/// columns) at one even pitch, 30, 36 or 44 of them in a row.
fn row(dark: &dyn Fn(usize, usize) -> bool, w: usize, y0: usize, y1: usize) -> Option<Row> {
    let hgt = y1 - y0;
    let need = (hgt / 10).max(1);
    let inked: Vec<bool> = (0..w).map(|x| (y0..y1).filter(|&y| dark(x, y)).count() >= need).collect();
    // runs of inked columns: (middle, width)
    let mut runs = vec![];
    let mut x = 0;
    while x < w {
        if !inked[x] {
            x += 1;
            continue;
        }
        let s = x;
        while x < w && inked[x] {
            x += 1;
        }
        runs.push(((s + x) as f64 / 2.0, (x - s) as f64));
    }
    if runs.len() < 20 {
        return None;
    }
    let mut d: Vec<f64> = runs.windows(2).map(|p| p[1].0 - p[0].0).collect();
    d.sort_by(f64::total_cmp);
    let pitch = d[d.len() / 2];
    if pitch < 4.0 {
        return None;
    }
    // the longest chain of glyph-wide runs, each a pitch or a few from the
    // last (a photo or a stamp beside the line is no glyph)
    let mut chain: Vec<f64> = vec![];
    let mut cur: Vec<f64> = vec![];
    for &(mid, wid) in &runs {
        let fits = wid <= 3.2 * pitch && cur.last().is_none_or(|&l| mid - l <= 3.6 * pitch);
        if !fits {
            if cur.len() > chain.len() {
                chain = std::mem::take(&mut cur);
            }
            cur.clear();
        }
        if wid <= 3.2 * pitch {
            cur.push(mid);
        }
    }
    if cur.len() > chain.len() {
        chain = cur;
    }
    if chain.len() < 15 {
        return None;
    }
    // the line's count: 30, 36 or 44 characters from its first glyph to its
    // last, the pitch their span over the count; the glyphs' middles fall on
    // it (a glyph narrower or wider than its cell moves its middle a little;
    // two or three run together put theirs between their cells)
    let (first, last) = (chain[0], *chain.last().unwrap());
    let gaps: Vec<f64> = chain.windows(2).map(|p| p[1] - p[0]).collect();
    let fit = |chars: usize| {
        let p = (last - first) / (chars - 1) as f64;
        let even = gaps.iter().filter(|&&g| ((2.0 * g / p).round() - 2.0 * g / p).abs() < 0.35).count();
        let ones = gaps.iter().filter(|&&g| (g / p - 1.0).abs() < 0.25).count();
        (even as f64 / gaps.len() as f64, ones as f64 / gaps.len() as f64, p)
    };
    let (chars, (even, ones, pitch)) = [30usize, 36, 44]
        .into_iter()
        .map(|c| (c, fit(c)))
        .filter(|(_, (_, _, p))| (p / pitch - 1.0).abs() < 0.2)
        .max_by(|a, b| (a.1 .0 + a.1 .1).total_cmp(&(b.1 .0 + b.1 .1)))?;
    if even < 0.85 || ones < 0.5 {
        return None;
    }
    // a glyph's height is about its pitch
    let tall = hgt as f64 / pitch;
    if !(0.6..=2.2).contains(&tall) {
        return None;
    }
    Some(Row { y0, y1, x0: first - pitch / 2.0, x1: last + pitch / 2.0, pitch, chars })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A document `w` x `h` px of paper, with lines of text above and, at
    /// its foot, `lines` lines of `chars` glyphs at `pitch` px (a glyph a
    /// box, a filler `<` a thin stroke; a few run into their neighbour).
    pub(crate) fn page(w: usize, h: usize, pitch: f64, chars: usize, lines: usize) -> Plane {
        let mut g = Plane { w, h, d: vec![0.92; w * h] };
        let mut ink = |x0: f64, y0: f64, x1: f64, y1: f64| {
            for y in y0.max(0.0) as usize..(y1 as usize).min(h) {
                for x in x0.max(0.0) as usize..(x1 as usize).min(w) {
                    g.d[y * w + x] = 0.1;
                }
            }
        };
        // proportional text: words of uneven letters
        let mut seed = 7u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            (seed >> 16) as f64 / 65536.0
        };
        let zone = h as f64 - (lines as f64 * 1.7 + 2.0) * pitch;
        for l in 0..8 {
            let y = 40.0 + l as f64 * 2.2 * pitch;
            if y + pitch > zone {
                break;
            }
            let mut x = 30.0;
            while x < w as f64 - 60.0 {
                let lw = pitch * (0.4 + 0.8 * rnd());
                ink(x, y, x + lw, y + pitch);
                x += lw + pitch * (0.15 + 0.9 * rnd() * rnd());
            }
        }
        let left = (w as f64 - pitch * chars as f64) / 2.0;
        for l in 0..lines {
            let y = h as f64 - (lines - l) as f64 * 1.7 * pitch - 0.6 * pitch;
            for c in 0..chars {
                let x = left + c as f64 * pitch;
                if (c * 7 + l) % 3 == 0 {
                    ink(x + 0.35 * pitch, y, x + 0.5 * pitch, y + 1.2 * pitch); // a filler
                } else if c % 11 == 4 {
                    ink(x + 0.15 * pitch, y, x + 1.05 * pitch, y + 1.2 * pitch); // runs into the next
                } else {
                    ink(x + 0.2 * pitch, y, x + 0.8 * pitch, y + 1.2 * pitch);
                }
            }
        }
        g
    }

    #[test]
    fn a_passports_zone_and_its_ruler() {
        // a passport's page at 8 px/mm: 2.54 mm = 20.32 px a character
        let g = page(1000, 704, 20.32, 44, 2);
        let m = find(&g).expect("the zone");
        assert_eq!((m.chars, m.lines), (44, 2));
        assert!((m.mm_per_px() - 0.125).abs() < 0.003, "{}", m.mm_per_px());
        // an ID-1 card's three lines of 30, a TD2's two of 36
        let m = find(&page(685, 432, 20.32, 30, 3)).expect("TD1");
        assert_eq!((m.chars, m.lines), (30, 3));
        let m = find(&page(840, 592, 20.32, 36, 2)).expect("TD2");
        assert_eq!((m.chars, m.lines), (36, 2));
    }

    #[test]
    fn no_zone_in_plain_text() {
        assert_eq!(find(&page(1000, 704, 20.32, 44, 0)), None);
        // a zone too narrow for its document is no zone of it
        assert_eq!(find(&page(2400, 704, 20.32, 44, 2)), None);
    }
}
