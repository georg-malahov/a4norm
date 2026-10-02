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
//!   something wide; it goes. Anything near black stays whatever its width
//!   (a logo, a bar): no shadow on paper is that dark.
//! - Under the mask the ink keeps its colour, divided by the light round
//!   it (a faded letter in the shadow comes back to its contrast), a pixel
//!   round it half so, and the rest is the paper's own tone: the brightest
//!   third of what lies round the mask (a cream sheet stays cream, white
//!   stays white). Outside the mask nothing changes.
//!
//! Where the light round a pixel is coloured, or the pixel itself is brown
//! or orange, it is a desk, not paper, and goes white with the shadow (no
//! print on paper is brown; a blue pen or a red stamp keeps its colour).
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
    // the light round each pixel per channel: where it is coloured (a brown
    // desk, not paper in shadow, which stays grey), nothing is print but
    // near-black
    let part = |p: &Plane| Plane { w: rw, h: rh, d: (y0..y1).flat_map(|y| p.d[y * w + x0..y * w + x1].to_vec()).collect() };
    let bgc: Vec<Plane> = img.c.iter().map(|p| closing(&part(p), r)).collect();
    let paper_like = |j: usize| {
        let (lo, hi) = bgc.iter().fold((1f32, 0f32), |(lo, hi), p| (lo.min(p.d[j]), hi.max(p.d[j])));
        hi - lo < GREY
    };
    // a brown or orange pixel is a desk (wood, cork, a table's edge in the
    // sheet's shadow): nothing printed on paper is that colour (a blue pen
    // or a red stamp is not)
    let brown = |j: usize| {
        let i = (y0 + j / rw) * w + x0 + j % rw;
        let (r, g, b) = (img.c[0].d[i], img.c[1.min(img.c.len() - 1)].d[i], img.c[2.min(img.c.len() - 1)].d[i]);
        r >= g && g >= b && r - b > 0.08 && luma.d[j] < 0.75
    };
    let mut ink: Vec<bool> = hat.iter().enumerate().map(|(j, &v)| v > thr && paper_like(j) && !brown(j)).collect();
    // ink thicker than a stroke is the rim of something wide
    drop_thick(&mut ink, rw, rh, STROKE_MM * px_mm / 2.0);
    for (j, k) in ink.iter_mut().enumerate() {
        let i = (y0 + j / rw) * w + x0 + j % rw;
        let (lo, hi) = img.c.iter().fold((1f32, 0f32), |(lo, hi), p| (lo.min(p.d[i]), hi.max(p.d[i])));
        *k |= luma.d[j] < BLACK && hi - lo < GREY;
    }
    let near: Vec<bool> = (0..rw * rh)
        .map(|i| {
            let (x, y) = (i % rw, i / rw);
            !ink[i] && (x.saturating_sub(1)..(x + 2).min(rw)).any(|xx| (y.saturating_sub(1)..(y + 2).min(rh)).any(|yy| ink[yy * rw + xx]))
        })
        .collect();
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
            let b = if bg.d[j] >= 0.5 { bg.d[j] } else { 1.0 };
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
/// their edge (a chamfer distance) are taken out whole.
fn drop_thick(ink: &mut [bool], w: usize, h: usize, half: f64) {
    let big = u32::MAX / 4;
    let mut d: Vec<u32> = ink.iter().map(|&k| if k { big } else { 0 }).collect();
    // 3-4 chamfer, two passes
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
    let lim = (half * 3.0) as u32;
    let mut seen = vec![false; w * h];
    let mut stack = vec![];
    for s in 0..w * h {
        if !ink[s] || seen[s] {
            continue;
        }
        seen[s] = true;
        stack.push(s);
        let mut part = vec![];
        let mut deep = 0u32;
        while let Some(i) = stack.pop() {
            part.push(i);
            deep = deep.max(d[i]);
            let (x, y) = (i % w, i / w);
            let mut go = |j: usize| {
                if ink[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 { go(i - 1) }
            if x + 1 < w { go(i + 1) }
            if y > 0 { go(i - w) }
            if y + 1 < h { go(i + w) }
        }
        if deep > lim {
            for i in part {
                ink[i] = false;
            }
        }
    }
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
}
