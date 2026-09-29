//! Where a form is filled in, from the picture of its page alone: writing
//! lines (dotted or solid), check boxes (square or round), fields (a line
//! with a stroke rising at each end, closed on top or not) and rows of
//! cells for one character each (comb: tax ID, IBAN, dates).
//!
//! Everything is built from strokes:
//! - horizontal strokes are runs of ink along a row, dots joined across
//!   gaps of up to 2.2 pt, kept where they are thin across;
//! - vertical strokes are unbroken runs down a column, kept where thin.
//!
//! A horizontal stroke with vertical ones standing on it is the bottom of a
//! row of cells: narrow, even cells side by side make a comb, others are
//! fields. One with nothing standing on its ends is a writing line if it is
//! long enough. Check boxes are the small enclosed white areas with a thin
//! border. Sizes are in points (`px_pt` pixels each), so any resolution
//! works; the rules were set at 200 dpi.
//!
//! Every candidate gets a number in reading order: rows by their bottom
//! edge, 6 pt apart at least, then left to right. The same page read from
//! another picture gives the same numbers as long as the same candidates
//! are found.
//!
//! One implementation for both modules: A4Norm Forms' OCR module
//! (a4norm-ocr) finds a page's candidates with it, and the scanner's
//! browser module (a4norm-rs) asks `looks_like_form` of a page it has just
//! made, to offer filling it in (D24), without the OCR module.

use image::RgbImage;

/// A writing line: from `(x0, y0)` to `(x1, y1)`, through the middle of the
/// stroke.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WLine {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// What a candidate is. Boxes are `[x0, y0, x1, y1]`: the white inside of
/// a field, a cell row or a check box, within its strokes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Line(WLine),
    Rect([f32; 4]),
    Comb([f32; 4], usize),
    Box([f32; 4]),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// 1, 2, … in reading order
    pub id: usize,
    pub kind: Kind,
}

impl Candidate {
    /// `[x0, y0, x1, y1]` around it.
    pub fn bounds(&self) -> [f32; 4] {
        match self.kind {
            Kind::Line(l) => [l.x0, l.y0.min(l.y1), l.x1, l.y0.max(l.y1)],
            Kind::Rect(b) | Kind::Comb(b, _) | Kind::Box(b) => b,
        }
    }
}

/// A word read on the page: its box, and each character with where across
/// the page it was read (empty when that is not known).
#[derive(Debug, Clone, Default)]
pub struct Text {
    pub bbox: [f32; 4],
    pub chars: Vec<(char, f32)>,
}

/// A page's candidates, in pixels, and the typical height of its fields.
#[derive(Debug, Clone)]
pub struct Geometry {
    pub candidates: Vec<Candidate>,
    pub typical_field_height: f32,
}

/// Rules in points (a 200 dpi page has 200/72 pixels to the point).
const DOT_GAP: f32 = 2.2; // dots of a dotted line joined across this
const H_RUN: f32 = 4.0; // a horizontal stroke is at least this long
const V_RUN: f32 = 3.0; // and a vertical one
const THIN: f32 = 2.5; // strokes are at most this thick
const RULE: f32 = 1.5; // a cell's sides are rules, thinner than a letter's stem
const LINE_MIN: f32 = 32.0; // a writing line
const FIT_STD: f32 = 1.3; // the stroke's pixels about its fitted line
const FOOT: f32 = 2.5; // a stroke stands on a line within this
const CELL_H: f32 = 9.0; // a cell's least height
const CELL_W: f32 = 5.0; // and width
const FIELD_MAX: f32 = 90.0; // a field's greatest height
const BOX_MIN: f32 = 2.8; // a check box's inside, least and greatest side
const BOX_MAX: f32 = 26.0;
const ROW: f32 = 6.0; // reading order: rows at least this far apart

/// The candidates on a page whose points are `px_pt` pixels. `text` are
/// the words read on it, if any: a cell inside a word is a letter.
pub fn find(page: &RgbImage, px_pt: f32, words: &[Text]) -> Geometry {
    let text: Vec<[f32; 4]> = words.iter().map(|t| t.bbox).collect();
    let (w, h) = (page.width() as usize, page.height() as usize);
    let pt = |v: f32| (v * px_pt).round().max(1.0) as usize;
    let ink = ink(page);
    // text sitting right on a stroke makes it a baseline or a heading's rule
    let hs: Vec<Stroke> = strokes(&ink, w, h, true, pt(DOT_GAP), pt(H_RUN), pt(THIN), px_pt)
        .into_iter()
        .filter(|s| !text_on(&ink, w, h, s, px_pt, -1.0) && !text_on(&ink, w, h, s, px_pt, 1.0))
        .collect();
    let vs = strokes(&ink, w, h, false, 1, pt(V_RUN), pt(THIN), px_pt);

    let mut found: Vec<Kind> = vec![];
    let boxes: Vec<[f32; 4]> = check_boxes(&ink, w, h, px_pt)
        .into_iter()
        .filter(|b| {
            let gap = gap_right(&ink, w, b, px_pt);
            !words.iter().any(|t| a_loop(b, t, gap, px_pt))
        })
        .collect();

    // what stands on each horizontal stroke
    let foot = FOOT * px_pt;
    let mut cells: Vec<[f32; 4]> = vec![];
    let mut lines = vec![];
    for s in &hs {
        let mut ticks: Vec<(f32, f32)> = vs
            .iter()
            .filter(|v| {
                let x = v.at(v.b1);
                (v.b1 - s.at(x)).abs() <= foot && x >= s.a0 - foot && x <= s.a1 + foot && v.b1 - v.b0 >= CELL_H * px_pt && v.t <= RULE * px_pt
            })
            .map(|v| (v.at(v.b1), v.b0))
            .collect();
        ticks.sort_by(|a, b| a.0.total_cmp(&b.0));
        // a doubled stroke (a thicker rule drawn as two) counts once
        ticks.dedup_by(|b, a| b.0 - a.0 < 1.5 * px_pt);
        let ends = ticks.len() >= 2
            && ticks.first().unwrap().0 <= s.a0 + foot
            && ticks.last().unwrap().0 >= s.a1 - foot;
        if ends {
            for p in ticks.windows(2) {
                let (x0, x1) = (p[0].0, p[1].0);
                let top = p[0].1.max(p[1].1); // the shorter stroke's top
                let y1 = s.at((x0 + x1) / 2.0);
                cells.push([x0, top, x1, y1]);
            }
        } else if s.a1 - s.a0 >= LINE_MIN * px_pt && !hung(s, &vs, foot) {
            lines.push(*s);
        }
    }
    // cells: the inside, within half a stroke; runs of narrow even ones side
    // by side are a comb
    let half = 0.6 * px_pt;
    let cells: Vec<[f32; 4]> = cells
        .into_iter()
        .map(|c| [c[0] + half, c[1] + half, c[2] - half, c[3] - half])
        .filter(|c| c[2] - c[0] >= CELL_W * px_pt && c[3] - c[1] >= CELL_H * px_pt && c[3] - c[1] <= FIELD_MAX * px_pt)
        // a letter's counter (the inside of a U) is no cell
        .filter(|c| !text.iter().any(|t| overlap(c, t) >= 0.6))
        .collect();
    let mut used = vec![false; cells.len()];
    let mut order: Vec<usize> = (0..cells.len()).collect();
    order.sort_by(|&a, &b| cells[a][0].total_cmp(&cells[b][0]));
    for &i in &order {
        if used[i] || !narrow(&cells[i]) {
            continue;
        }
        let mut run = vec![i];
        loop {
            let last = cells[*run.last().unwrap()];
            let next = order.iter().copied().find(|&j| {
                let c = cells[j];
                !used[j] && !run.contains(&j) && narrow(&c)
                    && (c[0] - last[2]).abs() <= 2.0 * half + px_pt
                    && (c[3] - last[3]).abs() <= px_pt
                    && ((c[2] - c[0]) / (last[2] - last[0]) - 1.0).abs() <= 0.25
            });
            match next {
                Some(j) => run.push(j),
                None => break,
            }
        }
        if run.len() >= 2 {
            for &j in &run {
                used[j] = true;
            }
            let (a, b) = (cells[run[0]], cells[*run.last().unwrap()]);
            let top = run.iter().map(|&j| cells[j][1]).fold(f32::MAX, f32::min);
            let comb = [a[0], top, b[2], a[3].max(b[3])];
            // a doubled bottom rule finds the same row twice
            if !found.iter().any(|k| matches!(k, Kind::Comb(o, _) if overlap(&comb, o) > 0.5)) {
                found.push(Kind::Comb(comb, run.len()));
            }
        }
    }
    // a comb's cells are enclosed too, but are not check boxes
    for b in &boxes {
        if !found.iter().any(|k| matches!(k, Kind::Comb(o, _) if overlap(b, o) > 0.5)) {
            found.push(Kind::Box(*b));
        }
    }
    for (i, c) in cells.iter().enumerate() {
        // a cell that is a check box, or holds candidates of its own (a
        // frame around a group), is not a field
        if used[i] || boxes.iter().any(|b| overlap(b, c) > 0.5) {
            continue;
        }
        if found.iter().any(|k| inside(&bounds(k), c) || matches!(k, Kind::Rect(o) if overlap(c, o) > 0.5)) {
            continue;
        }
        // a small frame with something in its middle: a section's number
        let small = c[2] - c[0] <= 30.0 * px_pt && c[3] - c[1] <= 30.0 * px_pt;
        if small && !blank_middle(&ink, w, c) {
            continue;
        }
        found.push(Kind::Rect(*c));
    }
    for l in join(lines, &ink, w, px_pt) {
        // a field's bottom rule drawn twice
        let bottom = |r: &[f32; 4]| (l.y0 - r[3]).abs() <= 3.0 * px_pt && l.x0 >= r[0] - 3.0 * px_pt && l.x1 <= r[2] + 3.0 * px_pt;
        // the tops of a heading's letters: a line through a word's upper
        // part; a value written on a line has the line at its foot
        let bounds = [l.x0, l.y0.min(l.y1) - 1.0, l.x1, l.y0.max(l.y1) + 1.0];
        let in_word = text.iter().any(|t| overlap(&bounds, t) >= 0.6 && (l.y0 + l.y1) / 2.0 < t[1] + 0.75 * (t[3] - t[1]));
        if !in_word && !found.iter().any(|k| matches!(k, Kind::Rect(r) if bottom(r))) {
            found.push(Kind::Line(l));
        }
    }
    // a frame around a group of candidates, or around text (a note in a
    // box), is not a field; a label inside a field takes less of it
    let all = found.clone();
    found.retain(|k| match k {
        Kind::Rect(r) => !all.iter().any(|o| o != k && inside(&bounds(o), r)) && !a_note(r, &text),
        _ => true,
    });

    let typical_field_height = field_height(&found, &ink, w, px_pt);
    Geometry { candidates: number(found, px_pt), typical_field_height }
}

/// Whether strokes hang from both ends of `s`: it is the top of a box or a
/// table, not a line to write on.
fn hung(s: &Stroke, vs: &[Stroke], foot: f32) -> bool {
    let hangs = |x: f32| vs.iter().any(|v| (v.at(v.b0) - x).abs() <= foot && (v.b0 - s.at(x)).abs() <= foot);
    hangs(s.a0) && hangs(s.a1)
}

/// Writing lines, a line broken in two (dots lost in print or in cleaning)
/// made whole: pieces one after the other on the same height, less than
/// 16 pt apart with paper between them. Two lines with a label between
/// ("Telefon ...... Fax: ......") stay two.
fn join(mut lines: Vec<Stroke>, ink: &[bool], w: usize, px_pt: f32) -> Vec<WLine> {
    lines.sort_by(|a, b| a.a0.total_cmp(&b.a0));
    let clear = |x0: f32, x1: f32, y: f32| {
        let (y0, y1) = ((y - 12.0 * px_pt).max(0.0) as usize, (y - 1.6 * px_pt).max(0.0) as usize);
        (y0..y1).all(|yy| (x0.max(0.0) as usize..(x1 as usize).min(w)).all(|x| !ink[yy * w + x]))
    };
    let mut out: Vec<WLine> = vec![];
    for s in lines {
        let l = WLine { x0: s.a0, y0: s.at(s.a0), x1: s.a1, y1: s.at(s.a1) };
        let prev = out.iter_mut().find(|p| {
            let gap = l.x0 - p.x1;
            gap >= -px_pt && gap <= 16.0 * px_pt && (l.y0 - p.y1).abs() <= 1.2 * px_pt && (gap <= 8.0 * px_pt || clear(p.x1, l.x0, p.y1))
        });
        match prev {
            Some(p) => (p.x1, p.y1) = (l.x1, l.y1),
            None => out.push(l),
        }
    }
    out
}

fn narrow(c: &[f32; 4]) -> bool {
    c[2] - c[0] <= 1.3 * (c[3] - c[1])
}

fn bounds(k: &Kind) -> [f32; 4] {
    Candidate { id: 0, kind: *k }.bounds()
}

/// Whether box `b` is a letter's loop in word `t` ("6" in "635"): inside
/// the word, lower than it, with a letter or digit read in it. A row of
/// boxes read as one word with their labels ("□ männlich □ weiblich" as
/// "männlichweiblich") has nothing read in the boxes; a round one read as
/// a letter ("◯ ja" as "Oja") has its label after it, a word space away
/// (`gap`, paper beyond its right side), where a letter's next is closer
/// ("Ocupación").
fn a_loop(b: &[f32; 4], t: &Text, gap: f32, px_pt: f32) -> bool {
    if overlap(b, &t.bbox) <= 0.8 || b[3] - b[1] >= 0.6 * (t.bbox[3] - t.bbox[1]) {
        return false;
    }
    if t.chars.is_empty() {
        return true;
    }
    let read = t.chars.iter().any(|&(c, x)| x > b[0] && x < b[2] && c.is_alphanumeric());
    let label = t.chars.iter().skip_while(|&&(_, x)| x < b[2]).take(2).filter(|&&(c, _)| c.is_alphabetic()).count() == 2;
    read && !(label && gap >= 1.5 * px_pt)
}

/// The paper right of box `b` across its middle, beyond its side: up to
/// the next ink, at most 3 pt.
fn gap_right(ink: &[bool], w: usize, b: &[f32; 4], px_pt: f32) -> f32 {
    let (y0, y1) = ((b[1] + 0.2 * (b[3] - b[1])) as usize, (b[1] + 0.8 * (b[3] - b[1])) as usize);
    let dark = |x: usize| (y0..=y1).any(|y| ink[y * w + x]);
    let (mut x, end) = (b[2] as usize, w.min((b[2] + 6.0 * px_pt) as usize));
    while x < end && dark(x) {
        x += 1;
    }
    let from = x;
    while x < end && !dark(x) && ((x - from) as f32) < 3.0 * px_pt {
        x += 1;
    }
    (x - from) as f32
}

/// Whether `r` holds text rather than room for it: three lines of words or
/// more, or words over two thirds of it. A field's label takes a line.
fn a_note(r: &[f32; 4], text: &[[f32; 4]]) -> bool {
    let area = ((r[2] - r[0]) * (r[3] - r[1])).max(1.0);
    let mut within: Vec<&[f32; 4]> = text.iter().filter(|t| overlap(t, r) > 0.8).collect();
    let covered = within.iter().map(|t| (t[2] - t[0]) * (t[3] - t[1])).sum::<f32>() / area;
    within.sort_by(|a, b| (a[1] + a[3]).total_cmp(&(b[1] + b[3])));
    let mut rows = 0;
    let mut last = f32::MIN;
    for t in within {
        let (mid, hgt) = ((t[1] + t[3]) / 2.0, t[3] - t[1]);
        if mid - last > hgt / 2.0 {
            rows += 1;
            last = mid;
        }
    }
    rows >= 3 || covered > 0.65
}

/// Whether `a`'s middle is inside `b`.
fn inside(a: &[f32; 4], b: &[f32; 4]) -> bool {
    let (x, y) = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0);
    a != b && x > b[0] && x < b[2] && y > b[1] && y < b[3]
}

/// The share of `a` that `b` covers.
fn overlap(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    w * h / ((a[2] - a[0]) * (a[3] - a[1])).max(1.0)
}

/// Reading order: by the bottom edge, a new row where it drops by ROW or
/// more from the row's first, then left to right; numbered from 1.
fn number(mut found: Vec<Kind>, px_pt: f32) -> Vec<Candidate> {
    found.sort_by(|a, b| bounds(a)[3].total_cmp(&bounds(b)[3]));
    let mut rows: Vec<Vec<Kind>> = vec![];
    for k in found {
        match rows.last_mut() {
            Some(r) if bounds(&k)[3] - bounds(&r[0])[3] < ROW * px_pt => r.push(k),
            _ => rows.push(vec![k]),
        }
    }
    let mut out = vec![];
    for mut r in rows {
        r.sort_by(|a, b| bounds(a)[0].total_cmp(&bounds(b)[0]));
        for k in r {
            out.push(Candidate { id: out.len() + 1, kind: k });
        }
    }
    out
}

/// Ink: darker than 70 % of the paper (the 95th percentile of brightness).
fn ink(page: &RgbImage) -> Vec<bool> {
    let gray: Vec<u8> = page.pixels().map(|p| ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8).collect();
    let mut hist = [0usize; 256];
    gray.iter().for_each(|&g| hist[g as usize] += 1);
    let (mut acc, mut paper) = (0, 255);
    for (v, n) in hist.iter().enumerate() {
        acc += n;
        if acc * 100 >= gray.len() * 95 {
            paper = v;
            break;
        }
    }
    let t = (paper as f32 * 0.7) as u8;
    gray.iter().map(|&g| g < t).collect()
}

/// A stroke: along `a` (x for horizontal, y for vertical) from `a0` to `a1`,
/// across it `b = c + m a` fitted; `b0`/`b1` its extent across at the ends
/// (for a vertical stroke: top and bottom).
#[derive(Debug, Clone, Copy)]
struct Stroke {
    a0: f32,
    a1: f32,
    c: f32,
    m: f32,
    b0: f32,
    b1: f32,
    /// mean thickness across, pixels
    t: f32,
}

impl Stroke {
    /// Across, at `a` along.
    fn at(&self, a: f32) -> f32 {
        self.c + self.m * a
    }
}

/// Strokes one way: runs of ink along it at least `run` long, joined across
/// gaps of `gap`; what is thicker across than `thin` dropped; the rest in
/// 8-connected pieces, each fitted with a line and kept if its pixels hug
/// the line.
#[allow(clippy::too_many_arguments)]
fn strokes(ink: &[bool], w: usize, h: usize, horizontal: bool, gap: usize, run: usize, thin: usize, px_pt: f32) -> Vec<Stroke> {
    // along: index a in 0..len_a, across: b in 0..len_b
    let (len_a, len_b) = if horizontal { (w, h) } else { (h, w) };
    let at = |a: usize, b: usize| if horizontal { b * w + a } else { a * w + b };
    let mut mask = vec![false; w * h];
    for b in 0..len_b {
        let mut a = 0;
        while a < len_a {
            if !ink[at(a, b)] {
                a += 1;
                continue;
            }
            let (start, mut end, mut miss) = (a, a, 0);
            while a < len_a && miss <= gap {
                if ink[at(a, b)] {
                    end = a;
                    miss = 0;
                } else {
                    miss += 1;
                }
                a += 1;
            }
            if end + 1 - start >= run {
                (start..=end).for_each(|i| mask[at(i, b)] = true);
            }
            a = end + 1;
        }
    }
    // drop what is thick across: runs of the mask across longer than `thin`
    for a in 0..len_a {
        let mut b = 0;
        while b < len_b {
            if !mask[at(a, b)] {
                b += 1;
                continue;
            }
            let s = b;
            while b < len_b && mask[at(a, b)] {
                b += 1;
            }
            if b - s > thin {
                (s..b).for_each(|i| mask[at(a, i)] = false);
            }
        }
    }
    // pieces
    let mut seen = vec![false; w * h];
    let mut out = vec![];
    let mut stack = vec![];
    for i0 in 0..w * h {
        if !mask[i0] || seen[i0] {
            continue;
        }
        seen[i0] = true;
        stack.push(i0);
        let (mut n, mut sa, mut sb, mut saa, mut sab) = (0f64, 0f64, 0f64, 0f64, 0f64);
        let mut px: Vec<(f32, f32)> = vec![];
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            let (a, b) = if horizontal { (x as f64, y as f64) } else { (y as f64, x as f64) };
            (n, sa, sb, saa, sab) = (n + 1.0, sa + a, sb + b, saa + a * a, sab + a * b);
            px.push((a as f32, b as f32));
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if mask[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let (a0, a1) = px.iter().fold((f32::MAX, f32::MIN), |(l, r), p| (l.min(p.0), r.max(p.0)));
        let t = n as f32 / (a1 - a0 + 1.0);
        if a1 - a0 + 1.0 < run as f32 {
            continue;
        }
        let var = saa / n - (sa / n).powi(2);
        let m = if var > 0.0 { ((sab / n - sa / n * sb / n) / var) as f32 } else { 0.0 };
        let c = (sb / n) as f32 - m * (sa / n) as f32;
        let std = (px.iter().map(|p| (p.1 - c - m * p.0).powi(2)).sum::<f32>() / n as f32).sqrt();
        if std > FIT_STD * px_pt || m.abs() > 0.1 {
            continue;
        }
        let (b0, b1) = if horizontal {
            (c + m * a0, c + m * a1)
        } else {
            // a vertical stroke's top and bottom, along y
            (a0, a1 + 1.0)
        };
        let (a0, a1) = if horizontal { (a0, a1 + 1.0) } else { (c + m * a0, c + m * a1) };
        out.push(if horizontal {
            Stroke { a0, a1, c: c + 0.5, m, b0, b1, t }
        } else {
            // for a vertical stroke, `at` gives x at a given y
            Stroke { a0: a0.min(a1), a1: a0.max(a1), c: c + 0.5, m, b0, b1, t }
        });
    }
    out
}

/// Check boxes: small white areas enclosed all round by a thin border,
/// square or round.
fn check_boxes(ink: &[bool], w: usize, h: usize, px_pt: f32) -> Vec<[f32; 4]> {
    let (lo, hi) = (BOX_MIN * px_pt, BOX_MAX * px_pt);
    let mut seen = vec![false; w * h];
    let mut out = vec![];
    let mut stack = vec![];
    for i0 in 0..w * h {
        if ink[i0] || seen[i0] {
            continue;
        }
        seen[i0] = true;
        stack.push(i0);
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (usize::MAX, usize::MAX, 0, 0, 0usize);
        let mut big = false;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1, n) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y), n + 1);
            big |= (x1 - x0) as f32 > hi || (y1.saturating_sub(y0)) as f32 > hi;
            let mut push = |j: usize| {
                if !ink[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 { push(i - 1) }
            if x + 1 < w { push(i + 1) }
            if y > 0 { push(i - w) }
            if y + 1 < h { push(i + w) }
        }
        let (bw, bh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
        if big || bw < lo || bh < lo || x0 == 0 || y0 == 0 || x1 + 1 == w || y1 + 1 == h {
            continue;
        }
        // square, or round (a circle fills 79 % of its square); a small
        // round one is the loop of a letter or digit ("6", "o"), a check box
        // that small is square ("□")
        let fill = n as f32 / (bw * bh);
        if (bw / bh - 1.0).abs() > 0.15 || fill < 0.72 || (fill < 0.88 && bw.min(bh) < 7.5 * px_pt) {
            continue;
        }
        // an empty box: a number or a letter in a frame is a label
        let blank = blank_middle(ink, w, &[x0 as f32, y0 as f32, x1 as f32, y1 as f32]);
        if blank && thin_border(ink, w, h, (x0 + x1) / 2, (y0 + y1) / 2, bw.min(bh), px_pt) {
            out.push([x0 as f32, y0 as f32, x1 as f32 + 1.0, y1 as f32 + 1.0]);
        }
    }
    out
}

/// Whether the middle half of `b` (a quarter in from each side) is paper.
fn blank_middle(ink: &[bool], w: usize, b: &[f32; 4]) -> bool {
    let (x0, y0, x1, y1) = (b[0] as usize, b[1] as usize, b[2] as usize, b[3] as usize);
    let (qx, qy) = ((x1 - x0) / 4, (y1 - y0) / 4);
    (y0 + qy..=y1 - qy).all(|y| (x0 + qx..=x1 - qx).all(|x| !ink[y * w + x]))
}

/// Whether the area around `(cx, cy)`, `side` across, is closed by a thin
/// border with paper beyond it, looking out four ways: a box's border is at
/// most a quarter of its side (0.6 to 2.4 pt: a cleaned scan's rules come
/// out bolder), and at most one side has
/// something within 0.8 pt beyond it. A letter's counter (a bold "o", an
/// "o" in a heading) has a thicker stroke, or its neighbours close on both
/// sides.
fn thin_border(ink: &[bool], w: usize, h: usize, cx: usize, cy: usize, side: f32, px_pt: f32) -> bool {
    let t = (0.25 * side).clamp(0.6 * px_pt, 2.4 * px_pt);
    let (max_t, gap) = (t.ceil() as i64, (0.8 * px_pt).ceil() as i64);
    let mut clear = 0;
    for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
        let (mut x, mut y) = (cx as i64, cy as i64);
        let inside = |x: i64, y: i64| x >= 0 && y >= 0 && x < w as i64 && y < h as i64;
        let dark = |x: i64, y: i64| inside(x, y) && ink[y as usize * w + x as usize];
        while inside(x, y) && !dark(x, y) {
            (x, y) = (x + dx, y + dy);
        }
        let mut t = 0;
        while dark(x, y) {
            (x, y, t) = (x + dx, y + dy, t + 1);
        }
        if t > max_t {
            return false;
        }
        let mut g = 0;
        while inside(x, y) && !dark(x, y) && g < gap {
            (x, y, g) = (x + dx, y + dy, g + 1);
        }
        if g >= gap {
            clear += 1;
        }
    }
    clear >= 3
}

/// Whether ink runs right along the stroke, above it (`side` -1) or below
/// (1), over most of its length: the letters of a line of text on its
/// baseline or under the tops of its small letters, or a heading on its
/// rule. A line to write on, a field's bottom, has paper above, and at most
/// a short label below.
fn text_on(ink: &[bool], w: usize, h: usize, s: &Stroke, px_pt: f32, side: f32) -> bool {
    let (x0, x1) = (s.a0 as usize, (s.a1 as usize).min(w));
    let (a, b) = (side * 1.6 * px_pt, side * 4.0 * px_pt);
    let (d0, d1) = (a.min(b), a.max(b));
    let (mut cols, mut touched) = (0, 0);
    // ink per row of the band: a rule drawn alongside (a doubled line) is
    // inked all along one row, text is not
    let mut rows = vec![0usize; (d1 - d0).ceil() as usize + 1];
    for x in (x0..x1).step_by(2) {
        let y = s.at(x as f32);
        cols += 1;
        let mut hit = false;
        for (k, r) in rows.iter_mut().enumerate() {
            let yy = (y + d0 + k as f32) as i64;
            if yy >= 0 && (yy as usize) < h && ink[yy as usize * w + x] {
                *r += 1;
                hit = true;
            }
        }
        touched += hit as usize;
    }
    let rule = rows.iter().any(|&r| r * 10 >= cols * 9);
    !rule && touched * 100 > cols * 55
}

/// The typical field's height: for a writing line the room above it (the
/// median over its length of the paper up to the first ink, a dotted line's
/// gaps closed, at most 40 pt),
/// for a field or a comb its inside, for a check box its side; the median.
fn field_height(found: &[Kind], ink: &[bool], w: usize, px_pt: f32) -> f32 {
    let mut hs: Vec<f32> = found
        .iter()
        .map(|k| match k {
            Kind::Line(l) => {
                let mut room: Vec<f32> = vec![];
                let (x0, x1) = (l.x0 as usize, (l.x1 as usize).min(w));
                for x in (x0 + (x1 - x0) / 10..x1 - (x1 - x0) / 10).step_by(4) {
                    let y = l.y0 + (l.y1 - l.y0) * (x - x0) as f32 / (x1 - x0).max(1) as f32;
                    let start = (y - 1.6 * px_pt) as i64;
                    let stop = (y - 40.0 * px_pt).max(0.0) as i64;
                    let mut yy = start;
                    let g = (DOT_GAP * px_pt) as usize;
                    let row = |yy: i64| (x.saturating_sub(g)..(x + g + 1).min(w)).any(|xx| ink[yy as usize * w + xx]);
                    while yy > stop && !row(yy) {
                        yy -= 1;
                    }
                    room.push(y - yy as f32);
                }
                room.sort_by(f32::total_cmp);
                room.get(room.len() / 2).copied().unwrap_or(0.0)
            }
            Kind::Rect(b) | Kind::Comb(b, _) | Kind::Box(b) => b[3] - b[1],
        })
        .collect();
    hs.sort_by(f32::total_cmp);
    hs.get(hs.len() / 2).copied().unwrap_or(0.0)
}

impl Geometry {
    /// The contract's fields, in points (`px_pt` pixels each): `lines`,
    /// `rects`, `combs`, `boxes` (each with its `id`), `typicalFieldHeight`.
    pub fn json_fields(&self, px_pt: f32) -> String {
        let p = |v: f32| format!("{:.2}", v / px_pt);
        let b = |b: &[f32; 4]| format!("[{},{},{},{}]", p(b[0]), p(b[1]), p(b[2]), p(b[3]));
        let (mut lines, mut rects, mut combs, mut boxes) = (vec![], vec![], vec![], vec![]);
        for c in &self.candidates {
            match c.kind {
                Kind::Line(l) => lines.push(format!(
                    "{{\"id\":{},\"x0\":{},\"y0\":{},\"x1\":{},\"y1\":{}}}",
                    c.id,
                    p(l.x0),
                    p(l.y0),
                    p(l.x1),
                    p(l.y1)
                )),
                Kind::Rect(r) => rects.push(format!("{{\"id\":{},\"box\":{}}}", c.id, b(&r))),
                Kind::Comb(r, n) => combs.push(format!("{{\"id\":{},\"box\":{},\"cells\":{n}}}", c.id, b(&r))),
                Kind::Box(r) => boxes.push(format!("{{\"id\":{},\"box\":{}}}", c.id, b(&r))),
            }
        }
        format!(
            "\"lines\":[{}],\"rects\":[{}],\"combs\":[{}],\"boxes\":[{}],\"typicalFieldHeight\":{}",
            lines.join(","),
            rects.join(","),
            combs.join(","),
            boxes.join(","),
            p(self.typical_field_height)
        )
    }
}

/// Whether a page looks like a blank form, for offering to fill it (D24):
/// its empty candidates and all of them, and how many of each kind. A
/// writing line counts when shorter than 400 pt (longer ones are a table's
/// rules or a letter's lines). Each is empty when under 4 % of what it
/// leaves for writing is dark (luma under 128):
/// - a field: its inside 1.5 pt in from the strokes, the lower 55 % of it
///   when it is higher than 14 pt (a printed label sits at the top);
/// - a comb: its inside, 1.5 pt in; a check box: its inside, 1 pt in;
/// - a line: the band over it, 0.7 of a typical field high (8–18 pt),
///   without the line itself.
///
/// The site offers the form when `empty >= 6` and `empty >= 0.6 * total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FormLook {
    pub empty: usize,
    pub total: usize,
    pub lines: usize,
    pub rects: usize,
    pub combs: usize,
    pub boxes: usize,
}

impl FormLook {
    /// `{"empty":…,"total":…,"lines":…,"rects":…,"combs":…,"boxes":…}`
    pub fn json(&self) -> String {
        format!(
            "{{\"empty\":{},\"total\":{},\"lines\":{},\"rects\":{},\"combs\":{},\"boxes\":{}}}",
            self.empty, self.total, self.lines, self.rects, self.combs, self.boxes
        )
    }
}

/// `looks_like_form` over a page's candidates found without words.
pub fn looks_like_form(page: &RgbImage, px_pt: f32) -> FormLook {
    let g = find(page, px_pt, &[]);
    let (w, h) = (page.width() as f32, page.height() as f32);
    // the share of dark pixels in `[x0, y0, x1, y1]`, in points
    let dark = |b: [f32; 4]| {
        let at = |v: f32, max: f32| (v * px_pt).round().clamp(0.0, max) as u32;
        let (x0, y0, x1, y1) = (at(b[0], w), at(b[1], h), at(b[2], w), at(b[3], h));
        let (mut n, mut d) = (0u32, 0u32);
        for y in y0..y1 {
            for x in x0..x1 {
                let [r, g, b] = page.get_pixel(x, y).0.map(f32::from);
                n += 1;
                d += (0.3 * r + 0.59 * g + 0.11 * b < 128.0) as u32;
            }
        }
        if n == 0 {
            0.0
        } else {
            d as f32 / n as f32
        }
    };
    let pt = |b: &[f32; 4], inset: f32| [b[0] / px_pt + inset, b[1] / px_pt + inset, b[2] / px_pt - inset, b[3] / px_pt - inset];
    let band = (0.7 * g.typical_field_height / px_pt).clamp(8.0, 18.0);
    let mut out = FormLook::default();
    for c in &g.candidates {
        let d = match c.kind {
            Kind::Line(l) => {
                let (x0, x1) = (l.x0.min(l.x1) / px_pt, l.x0.max(l.x1) / px_pt);
                if x1 - x0 >= 400.0 {
                    continue;
                }
                out.lines += 1;
                let y = l.y0.min(l.y1) / px_pt;
                dark([x0 + 2.0, y - band, x1 - 2.0, y - 1.5])
            }
            Kind::Rect(r) => {
                out.rects += 1;
                let b = pt(&r, 1.5);
                let hgt = b[3] - b[1];
                dark(if hgt > 14.0 { [b[0], b[1] + 0.45 * hgt, b[2], b[3]] } else { b })
            }
            Kind::Comb(r, _) => {
                out.combs += 1;
                dark(pt(&r, 1.5))
            }
            Kind::Box(r) => {
                out.boxes += 1;
                dark(pt(&r, 1.0))
            }
        };
        out.total += 1;
        out.empty += (d < 0.04) as usize;
    }
    out
}

/// The strokes found, as `(x0, y0, x1, y1, horizontal)`, for drawing.
#[doc(hidden)]
pub fn debug_strokes(page: &RgbImage, px_pt: f32) -> Vec<(f32, f32, f32, f32, bool)> {
    let (w, h) = (page.width() as usize, page.height() as usize);
    let pt = |v: f32| (v * px_pt).round().max(1.0) as usize;
    let ink = ink(page);
    let mut out = vec![];
    for s in strokes(&ink, w, h, true, pt(DOT_GAP), pt(H_RUN), pt(THIN), px_pt) {
        let text = text_on(&ink, w, h, &s, px_pt, -1.0) || text_on(&ink, w, h, &s, px_pt, 1.0);
        if !text {
            out.push((s.a0, s.at(s.a0), s.a1, s.at(s.a1), true));
        }
    }
    for s in strokes(&ink, w, h, false, 1, pt(V_RUN), pt(THIN), px_pt) {
        if s.t <= RULE * px_pt {
            out.push((s.at(s.b0), s.b0, s.at(s.b1), s.b1, false));
        }
    }
    out
}
