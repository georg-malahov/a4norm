//! Paper cleaned by hand (the site's brush or box): under a mask, a grey
//! shadow behind the print (a receipt's fold, a desk's edge) goes white and
//! the print stays. The print is told by the width of its strokes, not by
//! its tone: a stroke is thin, a shadow wide.
//!
//! - How much darker each pixel is than what lies round it within a
//!   stroke's reach: the black-hat, the luma's closing (max then min) by a
//!   window 1.2 mm across, less the luma. A wide shadow is as dark as its
//!   surroundings and scores nothing; a stroke on it scores what it stands
//!   out from it, on white paper or in the shadow alike.
//! - Ink where that is over the region's own noise (a robust spread of the
//!   black-hat there, times 4, at least 4 % of white).
//! - A part of that ink thicker than 1 mm across is no stroke: the rim of
//!   something wide; it goes. When only its faint rest is thick (small
//!   print joined into one blob by its soft rim, a shadow's grain grown
//!   onto letters), only that rest goes and the print stays. Anything near
//!   black stays whatever its width (a logo, a bar): no shadow on paper is
//!   that dark; and so does coloured ink well darker than the paper (a felt
//!   pen, bold blue print, a filled loop, a stamp): no shadow is coloured.
//! - Dust goes: the grey specks a shadow's grain leaves (a thermal
//!   receipt's fold), lighter than the print, small, grey and in no line
//!   and no dash (`drop_specks`). A second pass found them again before.
//! - Under the mask the ink keeps its colour, divided by the light round
//!   it (a faded letter in the shadow comes back to its contrast), a pixel
//!   round it half so, and the rest is the paper's own tone: the brightest
//!   third of what lies round the mask (a cream sheet stays cream, white
//!   stays white). Outside the mask nothing changes.
//!
//! Where the light round a pixel is coloured, read 3.6 mm wide, or the pixel
//! itself is brown or orange, it is a desk, not paper, and goes white with
//! the shadow (no print on paper is brown; a blue pen or a red stamp keeps
//! its colour). Colours are told against the paper's own tone: a beige or
//! cream sheet, or a warm light on a white one, is paper.
//!
//! What a fold has bleached to the shadow's own tone is not in the pixels
//! any more; it is left white. The site shows the result at once, to undo.

use crate::img::Img;
use crate::ops::Plane;

/// A stroke's reach: the closing's window, mm.
const WINDOW_MM: f64 = 1.2;
/// Ink thicker than this across is no stroke, mm.
const STROKE_MM: f64 = 1.0;
/// Darker than this, and all but grey, is ink whatever its width (a logo, a
/// black bar): no shadow on paper is near black. A brown desk is not grey.
const BLACK: f32 = 0.4;
const GREY: f32 = 0.08;
/// A paper tone no more coloured than this (beige, cream) is what colours
/// are told against.
const PAPER_TINT: f32 = 0.2;
/// Ink this coloured against the paper, and this much darker than the light
/// round it, is ink whatever its width.
const COLOUR: f32 = 0.16;
const COLOUR_DARK: f32 = 0.75;
/// The ground's colour is read this many times wider than a stroke's reach.
const DESK_K: usize = 3;
/// Dust is no more than this across, mm; a part of a line is no thicker
/// than `FLAT_MM` or as faint as dust, and the line's parts lie within
/// `GAP_MM` of each other along `LINE_MM` at least.
const SPECK_MM: f64 = 4.0;
const FLAT_MM: f64 = 1.0;
const GAP_MM: f64 = 2.5;
const LINE_MM: f64 = 8.0;
const DASH_MM: f64 = 1.2;
/// Lighter than print by this share of the way to paper is faint: dust is
/// further (0.7 of it and more on a receipt's fold), a faint logo's letter
/// or a faded dash nearer.
const FAINT: f32 = 0.6;

/// `img` cleaned under `mask` (true: clean; `img.w` x `img.h`), at `dpi`.
/// Returns the share of the masked pixels kept as ink.
pub fn clean_area(img: &mut Img, mask: &[bool], dpi: f64) -> f64 {
    let (w, h) = (img.w, img.h);
    assert_eq!(mask.len(), w * h);
    // the region: the mask's box and a window round it
    let px_mm = dpi / 25.4;
    let r = ((WINDOW_MM * px_mm / 2.0).ceil() as usize).max(1);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x0 >= x1 {
        return 0.0;
    }
    let m = 3 * r;
    let (x0, y0, x1, y1) = (x0.saturating_sub(m), y0.saturating_sub(m), (x1 + m).min(w), (y1 + m).min(h));
    let (rw, rh) = (x1 - x0, y1 - y0);
    let g = img.gray();
    let luma = Plane { w: rw, h: rh, d: (y0..y1).flat_map(|y| g.d[y * w + x0..y * w + x1].to_vec()).collect() };
    // the light round each pixel within a stroke's reach, and the black-hat
    let bg = closing(&luma, r);
    let hat: Vec<f32> = luma.d.iter().zip(&bg.d).map(|(&l, &b)| (b - l).max(0.0)).collect();
    // the region's noise: a robust spread of the black-hat under the mask
    let mut under: Vec<f32> = (0..rw * rh).filter(|&i| mask[(y0 + i / rw) * w + x0 + i % rw]).map(|i| hat[i]).collect();
    under.sort_by(f32::total_cmp);
    let med = under[under.len() / 2];
    let mut dev: Vec<f32> = under.iter().map(|v| (v - med).abs()).collect();
    dev.sort_by(f32::total_cmp);
    let sigma = 1.4826 * dev[dev.len() / 2];
    let thr = (med + 4.0 * sigma).max(0.04);
    // the paper's own tone (a cream sheet is not white): the brightest third
    // of what lies round the mask, or under it when it covers all
    let tone = |idx: &mut dyn Iterator<Item = usize>| -> Option<[f32; 3]> {
        let mut px: Vec<(f32, usize)> = idx.map(|i| (img.c.iter().map(|p| p.d[i]).sum::<f32>(), i)).collect();
        if px.len() < 200 {
            return None;
        }
        px.sort_by(|a, b| b.0.total_cmp(&a.0));
        px.truncate(px.len() / 3);
        let mut t = [1f32; 3];
        for (k, v) in t.iter_mut().enumerate() {
            let mut c: Vec<f32> = px.iter().map(|&(_, i)| img.c[k.min(img.c.len() - 1)].d[i]).collect();
            c.sort_by(f32::total_cmp);
            *v = c[c.len() / 2];
        }
        Some(t)
    };
    let region = |want: bool| (y0..y1).flat_map(move |y| (x0..x1).map(move |x| y * w + x)).filter(move |&i| mask[i] == want);
    let paper = tone(&mut region(false)).or_else(|| tone(&mut region(true))).unwrap_or([1.0; 3]);
    // colours are told against the paper's own: a beige or cream sheet, or
    // a warm light over a white one, is paper, not a desk. A tone too
    // coloured or too dark for paper (the mask is all on a desk) is not
    // balanced against
    let (tlo, thi) = paper.iter().fold((1f32, 0f32), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let white = if thi - tlo < PAPER_TINT && tlo > 0.45 { paper } else { [1.0; 3] };
    let rgb = |k: usize| k.min(img.c.len() - 1);
    let chroma = |v: [f32; 3]| {
        let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        hi - lo
    };
    let at = |j: usize| {
        let i = (y0 + j / rw) * w + x0 + j % rw;
        std::array::from_fn::<f32, 3, _>(|k| (img.c[rgb(k)].d[i] / white[k]).min(1.0))
    };
    // the light round each pixel per channel, as wide as a desk shows: where
    // it is coloured (a brown desk, not paper in shadow, which stays grey),
    // nothing is print but near-black. Within a stroke's reach a pen's own
    // colour would tint it (a signature's loops, the JPEG's colour bleeding)
    let part = |p: &Plane| Plane { w: rw, h: rh, d: (y0..y1).flat_map(|y| p.d[y * w + x0..y * w + x1].to_vec()).collect() };
    let bgc: Vec<Plane> = img.c.iter().map(|p| closing(&part(p), DESK_K * r)).collect();
    // (the white halo a sharpened stroke has is white, not tinted: balanced
    // values stop at white)
    let paper_like = |j: usize| chroma(std::array::from_fn(|k| (bgc[rgb(k)].d[j] / white[k]).min(1.0))) < GREY;
    // a brown or orange pixel is a desk (wood, cork, a table's edge in the
    // sheet's shadow): nothing printed on paper is that colour (a blue pen
    // or a red stamp is not)
    let brown = |j: usize| {
        let [r, g, b] = at(j);
        r >= g && g >= b && r - b > 0.08 && luma.d[j] < 0.75
    };
    let grey: Vec<bool> = (0..rw * rh).map(|j| chroma(at(j)) < GREY).collect();
    let mut ink: Vec<bool> = hat.iter().enumerate().map(|(j, &v)| v > thr && paper_like(j) && !brown(j)).collect();
    // ink thicker than a stroke is the rim of something wide; coloured ink
    // is not (a felt pen's loop filled in, a stamp)
    drop_thick(&mut ink, &hat, &grey, rw, rh, STROKE_MM * px_mm / 2.0);
    // near-black stays whatever its width (a logo, a bar), and so does
    // coloured ink well darker than the paper (a felt pen, bold blue print,
    // a stamp): no shadow is either, and a desk is brown
    let b = |j: usize| if bg.d[j] >= 0.5 { bg.d[j] } else { 1.0 };
    let rel: Vec<f32> = (0..rw * rh).map(|j| luma.d[j] / b(j)).collect();
    for (j, k) in ink.iter_mut().enumerate() {
        *k |= (luma.d[j] < BLACK && grey[j]) || (chroma(at(j)) >= COLOUR && rel[j] < COLOUR_DARK && !brown(j));
    }
    // dust: a grey speck the shadow's grain left, lighter than the print
    drop_specks(&mut ink, &rel, &grey, rw, rh, px_mm);
    let near: Vec<bool> = (0..rw * rh)
        .map(|i| {
            let (x, y) = (i % rw, i / rw);
            !ink[i] && (x.saturating_sub(1)..(x + 2).min(rw)).any(|xx| (y.saturating_sub(1)..(y + 2).min(rh)).any(|yy| ink[yy * rw + xx]))
        })
        .collect();
    let (mut masked, mut kept) = (0usize, 0usize);
    for yy in 0..rh {
        for xx in 0..rw {
            let (x, y) = (x0 + xx, y0 + yy);
            let i = y * w + x;
            if !mask[i] {
                continue;
            }
            masked += 1;
            let j = yy * rw + xx;
            // divided by the light round it where that is paper (in the
            // shadow too); on a dark ground (a desk's black edge) kept as it is
            let b = b(j);
            for (k, p) in img.c.iter_mut().enumerate() {
                let (v, t) = (p.d[i], paper[k.min(2)]);
                p.d[i] = t * if ink[j] {
                    (v / b).min(1.0)
                } else if near[j] {
                    ((v / b).min(1.0) + 1.0) / 2.0
                } else {
                    1.0
                };
            }
            kept += ink[j] as usize;
        }
    }
    if masked == 0 {
        0.0
    } else {
        kept as f64 / masked as f64
    }
}

/// Dust under the mask: the grain of a shadow (a thermal receipt's fold)
/// leaves grey specks the black-hat takes for print, and a second pass keeps
/// them again. A part of `ink` goes when it is
/// - lighter than print: its darkest point (`rel`, the luma divided by the
///   light round it) past `FAINT` of the way from the print's own (the
///   median over parts of half a square mm and more) to paper;
/// - grey (a blue pen or a red stamp is not dust) and no more than
///   `SPECK_MM` across (a pencil word or a signature is bigger);
/// - in no line: four parts or more, of one height and side by side
///   within `GAP_MM` on one centre line, `LINE_MM` long. Faint or flat
///   ones only, so a speck between letters is not saved by the letters: a
///   dotted or dashed rule, dark or faded, lives, and so does a whole faded
///   line of text;
/// - and no dash: along its own axes (askew too) no thicker than `FLAT_MM`,
///   `DASH_MM` long, two and a half times as long as thick and solid (a
///   rule a fold has thinned to a few dashes). Dust is round.
fn drop_specks(ink: &mut [bool], rel: &[f32], grey: &[bool], w: usize, h: usize, px_mm: f64) {
    struct Part {
        px: Vec<usize>,
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
        lo: f32,
        grey: bool,
        // the centre, and the length and thickness along the part's own
        // axes (a rule photographed askew is askew), the long one's direction
        c: (f64, f64),
        along: f64,
        across: f64,
        dir: (f64, f64),
    }
    let mut seen = vec![false; w * h];
    let mut parts = vec![];
    for s in 0..w * h {
        if !ink[s] || seen[s] {
            continue;
        }
        seen[s] = true;
        let mut stack = vec![s];
        let (mut px, mut x0, mut y0, mut x1, mut y1, mut lo, mut greys) = (vec![], w, h, 0, 0, 1f32, 0);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            lo = lo.min(rel[i]);
            greys += grey[i] as usize;
            px.push(i);
            for yy in y.saturating_sub(1)..(y + 2).min(h) {
                for xx in x.saturating_sub(1)..(x + 2).min(w) {
                    let j = yy * w + xx;
                    if ink[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let n = px.len() as f64;
        let (mx, my) = px.iter().fold((0.0, 0.0), |(a, b), &i| (a + (i % w) as f64 / n, b + (i / w) as f64 / n));
        let (sxx, syy, sxy) = px.iter().fold((0.0, 0.0, 0.0), |(a, b, c), &i| {
            let (dx, dy) = ((i % w) as f64 - mx, (i / w) as f64 - my);
            (a + dx * dx / n, b + dy * dy / n, c + dx * dy / n)
        });
        let (half, root) = ((sxx + syy) / 2.0, (((sxx - syy) / 2.0).powi(2) + sxy * sxy).sqrt());
        let t = 0.5 * (2.0 * sxy).atan2(sxx - syy);
        let grey = greys * 2 > px.len();
        parts.push(Part { px, x0, y0, x1, y1, lo, grey, c: (mx, my), along: (12.0 * (half + root)).sqrt(), across: (12.0 * (half - root)).max(1.0).sqrt(), dir: (t.cos(), t.sin()) });
    }
    let mut los: Vec<f32> = parts.iter().filter(|p| p.px.len() as f64 >= 0.5 * px_mm * px_mm).map(|p| p.lo).collect();
    if los.is_empty() {
        return;
    }
    los.sort_by(f32::total_cmp);
    let mid = los[los.len() / 2] + FAINT * (1.0 - los[los.len() / 2]);
    let faint: Vec<bool> = parts.iter().map(|p| p.lo > mid).collect();
    let (flat, gap, long) = (FLAT_MM * px_mm, GAP_MM * px_mm, LINE_MM * px_mm);
    let dash = |p: &Part| p.across <= flat && p.along >= 2.5 * p.across && p.px.len() as f64 >= 0.95 * p.along * p.across;
    fn find(r: &mut [usize], k: usize) -> usize {
        let mut k = k;
        while r[k] != k {
            r[k] = r[r[k]];
            k = r[k];
        }
        k
    }
    // one of a line with the other: side by side on one centre line, as a
    // rule's dots or a line's letters; or one a dash and the other a piece
    // of one on its axis, as thin and turned its way (a dashed rule askew)
    let span = |p: &Part, across: bool| if across { ((p.y0, p.y1), (p.x0, p.x1)) } else { ((p.x0, p.x1), (p.y0, p.y1)) };
    let side_by_side = |k: usize, across: bool| faint[k] || { let (_, (c0, c1)) = span(&parts[k], across); (c1 - c0) as f64 <= flat };
    let linked = |a: usize, c: usize, across: bool| {
        let (a, c, ka, kc) = (&parts[a], &parts[c], a, c);
        let (((a0, a1), (ac0, ac1)), ((c0, c1), (cc0, cc1))) = (span(a, across), span(c, across));
        let (ha, hc) = ((ac1 - ac0) as f64, (cc1 - cc0) as f64);
        let off = ((ac0 + ac1) as f64 - (cc0 + cc1) as f64).abs() / 2.0;
        let apart = (c0.max(a0) as f64 - c1.min(a1) as f64).max(0.0);
        if side_by_side(ka, across) && side_by_side(kc, across) && apart <= gap && off <= 0.35 * ha.min(hc) && ha.max(hc) <= 1.5 * ha.min(hc) {
            return true;
        }
        let (a, c) = if dash(a) { (a, c) } else { (c, a) };
        let askew = c.along >= 2.0 * c.across && (a.dir.0 * c.dir.0 + a.dir.1 * c.dir.1).abs() < 0.985;
        let piece = c.along >= 0.5 * a.along && c.along >= 1.5 * c.across;
        if !dash(a) || !piece || c.across > flat || a.across.max(c.across) > 1.5 * a.across.min(c.across) || askew {
            return false;
        }
        let (dx, dy) = (c.c.0 - a.c.0, c.c.1 - a.c.1);
        let (on, side) = ((dx * a.dir.0 + dy * a.dir.1).abs(), (dx * a.dir.1 - dy * a.dir.0).abs());
        side <= 0.5 * a.across.max(c.across) && on - (a.along + c.along) / 2.0 <= gap
    };
    let cand: Vec<usize> = (0..parts.len()).filter(|&k| side_by_side(k, false) || side_by_side(k, true) || dash(&parts[k])).collect();
    // along x, then along y (a rule standing up)
    let mut in_line = vec![false; parts.len()];
    for across in [false, true] {
        let mut root: Vec<usize> = (0..parts.len()).collect();
        let key = |k: usize| if across { parts[k].y0 } else { parts[k].x0 };
        let end = |k: usize| if across { parts[k].y1 } else { parts[k].x1 };
        let mut idx = cand.clone();
        idx.sort_by_key(|&k| key(k));
        for (n, &a) in idx.iter().enumerate() {
            for &c in &idx[n + 1..] {
                if key(c) as f64 > end(a) as f64 + gap {
                    break;
                }
                if linked(a, c, across) {
                    let (ra, rc) = (find(&mut root, a), find(&mut root, c));
                    root[ra] = rc;
                }
            }
        }
        let mut lines: std::collections::HashMap<usize, (usize, usize, usize, usize, usize)> = Default::default();
        for &k in &cand {
            let p = &parts[k];
            let e = lines.entry(find(&mut root, k)).or_insert((0, w, h, 0, 0));
            *e = (e.0 + 1, e.1.min(p.x0), e.2.min(p.y0), e.3.max(p.x1), e.4.max(p.y1));
        }
        for &k in &cand {
            let (n, x0, y0, x1, y1) = lines[&find(&mut root, k)];
            in_line[k] |= n >= 4 && (x1 - x0).max(y1 - y0) as f64 >= long;
        }
    }
    for (k, p) in parts.iter().enumerate() {
        let long_side = (p.x1 - p.x0).max(p.y1 - p.y0) as f64;
        if faint[k] && p.grey && long_side <= SPECK_MM * px_mm && !in_line[k] && !(dash(p) && p.along >= DASH_MM * px_mm) {
            for &i in &p.px {
                ink[i] = false;
            }
        }
    }
}

/// Max then min over a (2r+1)-square window: a grey closing, separable.
fn closing(p: &Plane, r: usize) -> Plane {
    let dil = filter(p, r, f32::max);
    filter(&dil, r, f32::min)
}

fn filter(p: &Plane, r: usize, f: fn(f32, f32) -> f32) -> Plane {
    let (w, h) = (p.w, p.h);
    let mut a = vec![0f32; w * h];
    for y in 0..h {
        run(&p.d[y * w..(y + 1) * w], r, f, &mut a[y * w..(y + 1) * w]);
    }
    let mut b = vec![0f32; w * h];
    let (mut col, mut out) = (vec![0f32; h], vec![0f32; h]);
    for x in 0..w {
        for y in 0..h {
            col[y] = a[y * w + x];
        }
        run(&col, r, f, &mut out);
        for y in 0..h {
            b[y * w + x] = out[y];
        }
    }
    Plane { w, h, d: b }
}

/// `f` (max or min) over a window of 2r+1 along `v`, clipped at its ends:
/// van Herk / Gil-Werman, three passes whatever r. `v` is padded with what
/// `f` ignores, so every window is whole.
fn run(v: &[f32], r: usize, f: fn(f32, f32) -> f32, out: &mut [f32]) {
    let none = if f(0.0, 1.0) == 1.0 { f32::NEG_INFINITY } else { f32::INFINITY };
    let k = 2 * r + 1;
    let mut p = vec![none; v.len() + 2 * r];
    p[r..r + v.len()].copy_from_slice(v);
    let n = p.len();
    let mut g = vec![0f32; n];
    let mut hh = vec![0f32; n];
    for i in 0..n {
        g[i] = if i % k == 0 { p[i] } else { f(g[i - 1], p[i]) };
    }
    for i in (0..n).rev() {
        hh[i] = if i % k == k - 1 || i == n - 1 { p[i] } else { f(hh[i + 1], p[i]) };
    }
    // the window of v[i] is p[i..i + k]
    for (i, o) in out.iter_mut().enumerate() {
        *o = f(hh[i], g[i + k - 1]);
    }
}

/// Parts of `ink` whose deepest point lies further than `half` px from
/// their edge (a chamfer distance) are taken out, but for coloured ones
/// (mostly not `grey`). Whole, when their core, what is at least half as
/// dark as their darkest (`hat`), is as thick: the rim of something wide.
/// Else only what is not core goes: print with something faint round it,
/// the rim a clean page's low threshold lets in round small print (which
/// joins a word into one blob) or a shadow's grain grown onto the letters.
fn drop_thick(ink: &mut [bool], hat: &[f32], grey: &[bool], w: usize, h: usize, half: f64) {
    let mut part_of = vec![usize::MAX; w * h];
    let mut parts: Vec<Vec<usize>> = vec![];
    for s in 0..w * h {
        if !ink[s] || part_of[s] != usize::MAX {
            continue;
        }
        let k = parts.len();
        part_of[s] = k;
        let (mut stack, mut part) = (vec![s], vec![]);
        while let Some(i) = stack.pop() {
            part.push(i);
            let (x, y) = (i % w, i / w);
            let mut go = |j: usize| {
                if ink[j] && part_of[j] == usize::MAX {
                    part_of[j] = k;
                    stack.push(j);
                }
            };
            if x > 0 { go(i - 1) }
            if x + 1 < w { go(i + 1) }
            if y > 0 { go(i - w) }
            if y + 1 < h { go(i + w) }
        }
        parts.push(part);
    }
    let top: Vec<f32> = parts.iter().map(|p| p.iter().map(|&i| hat[i]).fold(0.0, f32::max)).collect();
    let core: Vec<bool> = (0..w * h).map(|i| ink[i] && hat[i] >= 0.5 * top[part_of[i]]).collect();
    let (whole, cored) = (chamfer(ink, w, h), chamfer(&core, w, h));
    let lim = (half * 3.0) as u32;
    for part in parts {
        let deep = |d: &[u32]| part.iter().map(|&i| d[i]).max().unwrap_or(0);
        if deep(&whole) <= lim || part.iter().filter(|&&i| grey[i]).count() * 2 <= part.len() {
            continue;
        }
        let all = deep(&cored) > lim;
        for i in part {
            if all || !core[i] {
                ink[i] = false;
            }
        }
    }
}

/// A 3-4 chamfer distance from the edge of `m`, two passes.
fn chamfer(m: &[bool], w: usize, h: usize) -> Vec<u32> {
    let big = u32::MAX / 4;
    let mut d: Vec<u32> = m.iter().map(|&k| if k { big } else { 0 }).collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if d[i] == 0 {
                continue;
            }
            let mut v = d[i];
            if x > 0 { v = v.min(d[i - 1] + 3) }
            if y > 0 { v = v.min(d[i - w] + 3) }
            if x > 0 && y > 0 { v = v.min(d[i - w - 1] + 4) }
            if x + 1 < w && y > 0 { v = v.min(d[i - w + 1] + 4) }
            if x == 0 || y == 0 || x + 1 == w { v = v.min(3) }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if d[i] == 0 {
                continue;
            }
            let mut v = d[i];
            if x + 1 < w { v = v.min(d[i + 1] + 3) }
            if y + 1 < h { v = v.min(d[i + w] + 3) }
            if x + 1 < w && y + 1 < h { v = v.min(d[i + w + 1] + 4) }
            if x > 0 && y + 1 < h { v = v.min(d[i + w - 1] + 4) }
            if y + 1 == h { v = v.min(3) }
            d[i] = v;
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A receipt at 150 dpi: lines of thin strokes on white; a fold's shadow
    /// across it (0.62 of white) with strokes in it and a faded one; a
    /// dotted rule; a brown desk at its side.
    fn receipt() -> (Img, Vec<(usize, usize, &'static str)>) {
        let (w, h) = (600, 420);
        let mut c = vec![vec![1f32; w * h], vec![1f32; w * h], vec![1f32; w * h]];
        let mut put = |x: usize, y: usize, rgb: [f32; 3]| {
            for k in 0..3 {
                c[k][y * w + x] = rgb[k];
            }
        };
        let mut probes = vec![];
        // the shadow: rows 150-250, soft over 12 px at its edges
        let shade = |y: usize| {
            let t = if y < 138 || y > 262 { 0.0 } else if y < 150 { (y - 138) as f32 / 12.0 } else if y > 250 { (262 - y) as f32 / 12.0 } else { 1.0 };
            1.0 - 0.38 * t
        };
        for y in 0..h {
            for x in 0..480 {
                let s = shade(y);
                put(x, y, [s, s, s]);
            }
        }
        // strokes: 2 px wide, 14 tall, every 9 px, rows of text
        for (row, tone) in [(60usize, 0.12f32), (180, 0.12), (215, 0.47), (300, 0.12)] {
            for k in 0..40 {
                let x = 20 + k * 11;
                for y in row..row + 14 {
                    for xx in x..x + 2 {
                        let s = if tone > 0.4 { tone } else { tone * shade(y) };
                        put(xx, y, [s, s, s]);
                    }
                }
            }
            probes.push((21, row + 7, if tone > 0.4 { "faded" } else { "ink" }));
        }
        probes.push((300, 160, "shadow"));
        probes.push((300, 240, "shadow"));
        // a dotted rule: 2x2 dots every 6 px, grey
        for k in 0..60 {
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                put(20 + k * 6 + dx, 340 + dy, [0.55; 3]);
            }
        }
        probes.push((20, 340, "dot"));
        // the desk, brown, beside the receipt
        for y in 0..h {
            for x in 480..w {
                put(x, y, [0.55, 0.42, 0.3]);
            }
        }
        probes.push((540, 100, "desk"));
        (Img::from_planes(c.into_iter().map(|d| Plane { w, h, d }).collect()), probes)
    }

    #[test]
    fn the_shadow_goes_the_print_stays() {
        let (mut img, probes) = receipt();
        let before = img.clone();
        // the brush over all but a strip at the top
        let (w, h) = (img.w, img.h);
        let mask: Vec<bool> = (0..w * h).map(|i| i / w >= 30).collect();
        clean_area(&mut img, &mask, 150.0);
        let g = img.gray();
        for (x, y, what) in probes {
            let v = g.d[y * w + x];
            match what {
                "ink" => assert!(v < 0.3, "{what} at {x},{y}: {v}"),
                "faded" => assert!(v < 0.85, "{what} at {x},{y}: {v}"),
                "dot" => assert!(v < 0.9, "{what} at {x},{y}: {v}"),
                _ => assert!(v > 0.97, "{what} at {x},{y}: {v}"),
            }
        }
        // nothing outside the mask moved
        assert!((0..30 * w).all(|i| (0..3).all(|k| img.c[k].d[i] == before.c[k].d[i])));
    }

    #[test]
    fn cream_paper_stays_cream() {
        // the same receipt on cream paper: the shadow goes to the paper's
        // tone round the mask, or under it when the mask covers all
        for whole in [false, true] {
            let (mut img, _) = receipt();
            let cream = [0.97f32, 0.97, 0.94];
            for (k, p) in img.c.iter_mut().enumerate() {
                p.d.iter_mut().for_each(|v| *v *= cream[k]);
            }
            let (w, h) = (img.w, img.h);
            let mask: Vec<bool> = (0..w * h).map(|i| whole || i / w >= 30).collect();
            clean_area(&mut img, &mask, 150.0);
            let i = 160 * w + 300; // in the shadow
            for k in 0..3 {
                assert!((img.c[k].d[i] - cream[k]).abs() < 0.015, "whole {whole}, channel {k}: {}", img.c[k].d[i]);
            }
        }
    }

    #[test]
    fn the_dust_goes_the_dots_stay() {
        // the shadow's grain: grey specks in it, lighter than the print and
        // strewn about; a full stop, a faded dashed rule, the dotted one and
        // the faded strokes are print
        let (mut img, probes) = receipt();
        let (w, h) = (img.w, img.h);
        let put = |img: &mut Img, x: usize, y: usize, n: usize, v: f32| {
            for yy in y..y + n {
                for xx in x..x + n {
                    for p in img.c.iter_mut() {
                        p.d[yy * w + xx] = v;
                    }
                }
            }
        };
        let mut specks = vec![];
        let mut seed = 7u32;
        for _ in 0..40 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            // in the shadow, off the rows of print
            let y = [153, 197, 232][(seed >> 12) as usize % 3] + (seed >> 20) as usize % 14;
            let (x, n) = (30 + (seed >> 8) as usize % 420, 2 + (seed >> 4) as usize % 3);
            put(&mut img, x, y, n, 0.62 * 0.78);
            specks.push((x + n / 2, y + n / 2));
        }
        put(&mut img, 455, 311, 3, 0.12);
        for k in 0..30 {
            for x in 20 + k * 12..28 + k * 12 {
                for y in 380..382 {
                    put(&mut img, x, y, 1, 0.8);
                }
            }
        }
        let mask: Vec<bool> = (0..w * h).map(|i| i / w >= 30).collect();
        clean_area(&mut img, &mask, 150.0);
        let g = img.gray();
        for (x, y) in specks {
            assert!(g.d[y * w + x] > 0.97, "speck at {x},{y}: {}", g.d[y * w + x]);
        }
        assert!(g.d[312 * w + 456] < 0.3, "full stop: {}", g.d[312 * w + 456]);
        assert!(g.d[380 * w + 24] < 0.95, "faded dash: {}", g.d[380 * w + 24]);
        for (x, y, what) in probes {
            let v = g.d[y * w + x];
            match what {
                "ink" => assert!(v < 0.3, "{what} at {x},{y}: {v}"),
                "faded" => assert!(v < 0.85, "{what} at {x},{y}: {v}"),
                "dot" => assert!(v < 0.9, "{what} at {x},{y}: {v}"),
                _ => {}
            }
        }
        // a second pass leaves it as it is
        let once = img.clone();
        clean_area(&mut img, &mask, 150.0);
        let diff = img.c.iter().zip(&once.c).flat_map(|(a, b)| a.d.iter().zip(&b.d).map(|(x, y)| (x - y).abs())).fold(0f32, f32::max);
        assert!(diff < 0.05, "second pass moved {diff}");
    }

    /// A signed page at 200 dpi on `paper`: a line of small grey print on
    /// the open paper, a shadow band (0.88) with a felt pen's blue stroke
    /// 1.3 mm wide, a bold blue name (strokes 1.5 mm), a filled "a" and
    /// small grey print in it. Small print has a soft rim, as a photo's has.
    fn signed(paper: [f32; 3]) -> (Img, Vec<(usize, usize, &'static str)>) {
        let (w, h) = (900, 400);
        let mut c: Vec<Vec<f32>> = (0..3).map(|k| vec![paper[k]; w * h]).collect();
        let shade = |y: usize| if (150..300).contains(&y) { 0.88 } else { 1.0 };
        for y in 0..h {
            for x in 0..w {
                for k in 0..3 {
                    c[k][y * w + x] *= shade(y);
                }
            }
        }
        let mut put = |x: usize, y: usize, rgb: [f32; 3], a: f32| {
            for k in 0..3 {
                let v = &mut c[k][y * w + x];
                *v = *v * (1.0 - a) + rgb[k] * shade(y) * a;
            }
        };
        let blue = [0.12, 0.23, 0.54];
        let mut probes = vec![];
        // small grey print: 2 px strokes 12 tall, 5 apart, a soft rim
        for (y0, grey) in [(100usize, 0.47f32), (270, 0.53)] {
            for k in 0..60 {
                let x0 = 40 + k * 5 + (k / 6) * 8;
                for y in y0 - 1..y0 + 13 {
                    for x in x0 - 1..x0 + 3 {
                        let inner = (y0..y0 + 12).contains(&y) && (x0..x0 + 2).contains(&x);
                        put(x, y, [grey; 3], if inner { 1.0 } else { 0.4 });
                    }
                }
            }
            probes.push((40, y0 + 6, "print"));
        }
        // a felt pen's stroke, 10 px wide
        for x in 50..400 {
            let yc = 210.0 + 30.0 * ((x as f32) / 40.0).sin();
            for y in (yc - 5.0) as usize..(yc + 5.0) as usize {
                put(x, y, blue, 1.0);
            }
        }
        probes.push((90, (210.0 + 30.0 * (90f32 / 40.0).sin()) as usize, "blue"));
        // a bold name: strokes 12 px wide, 50 tall
        for k in 0..4 {
            for y in 180..230 {
                for x in 450 + k * 30..462 + k * 30 {
                    put(x, y, blue, 1.0);
                }
            }
        }
        probes.push((456, 205, "blue"));
        // a filled "a"
        for y in 190..230 {
            for x in 620..660 {
                let (dx, dy) = ((x as f32 - 640.0) / 16.0, (y as f32 - 210.0) / 14.0);
                if dx * dx + dy * dy <= 1.0 {
                    put(x, y, blue, 1.0);
                }
            }
        }
        probes.push((640, 210, "blue"));
        probes.push((800, 170, "paper"));
        probes.push((300, 290, "paper"));
        (Img::from_planes(c.into_iter().map(|d| Plane { w, h, d }).collect()), probes)
    }

    #[test]
    fn a_signature_on_beige_or_white_stays() {
        for paper in [[0.984f32, 0.984, 0.973], [0.937, 0.886, 0.784]] {
            let (mut img, probes) = signed(paper);
            let (w, h) = (img.w, img.h);
            let mask: Vec<bool> = (0..w * h).map(|i| i / w >= 20).collect();
            clean_area(&mut img, &mask, 200.0);
            let g = img.gray();
            for (x, y, what) in probes {
                let i = y * w + x;
                let (r, b, v) = (img.c[0].d[i], img.c[2].d[i], g.d[i]);
                match what {
                    "blue" => assert!(v < 0.4 && b > r + 0.2, "{paper:?}: {what} at {x},{y}: {r} {b} {v}"),
                    "print" => assert!(v < 0.7, "{paper:?}: {what} at {x},{y}: {v}"),
                    _ => {
                        for k in 0..3 {
                            assert!((img.c[k].d[i] - paper[k]).abs() < 0.03, "{paper:?}: {what} at {x},{y}, channel {k}: {}", img.c[k].d[i]);
                        }
                    }
                }
            }
        }
    }
}
