//! Paper cleaned the way a phone scanner's "magic colour" filter does, for a
//! sheet only (`--magic`; a spread, a card and a photo keep their colour
//! copy). Two steps take the place of the flat-field and the tone stretch:
//!
//! 1. The paper's own light is measured where the paper is: the bright
//!    quantile of each block, blocks that are mostly print (a black bar, a
//!    photo) thrown out and filled from their neighbours. Dividing by it evens
//!    out a shadow with a sharp edge too, which one heavy blur of the whole
//!    page cannot follow.
//! 2. What is left is either ink or paper. Ink is darker than the paper by
//!    more than the paper's own grain there (a shadowed corner is grainier, so
//!    it needs a darker mark), and it has an edge next to it; a soft stain has
//!    none. Solid ink far from its edges (a black bar) is joined to the ink
//!    that has them. Everything else goes to white; ink keeps its tone,
//!    pressed a little darker.

use crate::img::Img;
use crate::ops::{self, blur, morph_k, Filter, Plane};

/// The paper's light across the page, one channel.
fn background(p: &Plane, block: usize) -> Plane {
    let (w, h) = (p.w, p.h);
    let (bw, bh) = (w.div_ceil(block), h.div_ceil(block));
    let mut grid = Plane::new(bw, bh);
    for by in 0..bh {
        for bx in 0..bw {
            let mut hist = [0u32; 256];
            let mut n = 0u32;
            for y in by * block..((by + 1) * block).min(h) {
                for &v in &p.row(y)[bx * block..((bx + 1) * block).min(w)] {
                    hist[ops::to8(v) as usize] += 1;
                    n += 1;
                }
            }
            let want = (n as f64 * BG_QUANTILE) as u32;
            let (mut acc, mut q) = (0u32, 255usize);
            for (v, &c) in hist.iter().enumerate() {
                acc += c;
                if acc > want {
                    q = v;
                    break;
                }
            }
            grid.d[by * bw + bx] = q as f32 / 255.0;
        }
    }
    // a block well below the bright blocks around it is print, not paper
    let near = Plane { w: bw, h: bh, d: morph_k(&grid.d, bw, bh, &ops::square(2), true) };
    let near = blur(&near, 1.5);
    let mut wgt: Vec<f32> = grid.d.iter().zip(&near.d).map(|(&v, &r)| (v >= BG_PRINT * r) as u8 as f32).collect();
    let mut val: Vec<f32> = grid.d.iter().zip(&wgt).map(|(&v, &k)| v * k).collect();
    // filled from further and further out, as normalized convolution
    for s in [1.0, 2.0, 4.0, 8.0] {
        let nv = blur(&Plane { w: bw, h: bh, d: val.clone() }, s);
        let nw = blur(&Plane { w: bw, h: bh, d: wgt.clone() }, s);
        for i in 0..val.len() {
            if wgt[i] == 0.0 && nw.d[i] > 1e-3 {
                val[i] = nv.d[i] / nw.d[i];
                wgt[i] = 1.0;
            }
        }
    }
    let grid = blur(&Plane { w: bw, h: bh, d: val }, 1.0);
    let big = ops::resize(&grid, bw * block, bh * block, Filter::Triangle);
    big.crop(0, 0, w, h)
}

/// Step 1: each channel divided by the paper's light in it.
pub fn divide_paper(img: &mut Img) {
    let block = (img.w.min(img.h) / 36).max(16);
    for p in img.c.iter_mut() {
        let bg = background(p, block);
        let w = p.w;
        ops::rows(&mut p.d, w, |y, row| {
            for (v, &b) in row.iter_mut().zip(bg.row(y)) {
                *v = (*v / b.max(0.05)).min(1.0);
            }
        });
    }
    img.q8();
}

/// 4-connected runs of `core` that hold at least one `seed` pixel.
fn grow(core: &[u8], seed: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let mut stack = vec![];
    for s in 0..w * h {
        if seed[s] == 0 || core[s] == 0 || out[s] != 0 {
            continue;
        }
        out[s] = 1;
        stack.push(s);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            let mut go = |j: usize| {
                if core[j] != 0 && out[j] == 0 {
                    out[j] = 1;
                    stack.push(j);
                }
            };
            if x > 0 {
                go(i - 1);
            }
            if x + 1 < w {
                go(i + 1);
            }
            if y > 0 {
                go(i - w);
            }
            if y + 1 < h {
                go(i + w);
            }
        }
    }
    out
}

/// Step 2: ink kept and pressed darker, everything else white. Returns the
/// share of the page kept as ink, in percent.
pub fn ink_or_paper(img: &mut Img) -> f64 {
    let (w, h) = (img.w, img.h);
    let n = img.gray();
    // the paper's grain around each pixel: its mean distance from white
    let paper: Vec<f32> = n.d.iter().map(|&v| (v > 0.8) as u8 as f32).collect();
    let dev: Vec<f32> = n.d.iter().zip(&paper).map(|(&v, &p)| (1.0 - v).abs() * p).collect();
    let dev = blur(&Plane { w, h, d: dev }, GRAIN_SIGMA);
    let cnt = blur(&Plane { w, h, d: paper.clone() }, GRAIN_SIGMA);
    let grain: Vec<f32> = dev.d.iter().zip(&cnt.d).map(|(&d, &c)| 1.25 * d / c.max(1e-3)).collect();
    let thr: Vec<f32> = grain.iter().map(|&g| 1.0 - (GRAIN_K * g).max(INK_MIN)).collect();
    // and its own level there, which a slow shade left after the division
    // keeps a little under white
    let lit: Vec<f32> = n.d.iter().zip(&paper).map(|(&v, &p)| v * p).collect();
    let lit = blur(&Plane { w, h, d: lit }, GRAIN_SIGMA);
    let level: Vec<f32> = lit.d.iter().zip(&cnt.d).map(|(&l, &c)| if c > 1e-3 { l / c } else { 1.0 }).collect();
    drop((dev, cnt, lit));
    // an edge next to it: Sobel of the slightly blurred page
    let b = blur(&n, 1.0);
    let mut edge = vec![0u8; w * h];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w - 1 {
            let at = |dx: isize, dy: isize| b.d[(y as isize + dy) as usize * w + (x as isize + dx) as usize];
            let gx = at(1, -1) + 2.0 * at(1, 0) + at(1, 1) - at(-1, -1) - 2.0 * at(-1, 0) - at(-1, 1);
            let gy = at(-1, 1) + 2.0 * at(0, 1) + at(1, 1) - at(-1, -1) - 2.0 * at(0, -1) - at(1, -1);
            edge[y * w + x] = (gx.hypot(gy) > EDGE) as u8;
        }
    }
    drop(b);
    let near = morph_k(&edge, w, h, &ops::disk(EDGE_REACH), true);
    drop(edge);
    // a faint rule: averaged along a row or a column the grain goes, the rule
    // stays
    let along = |horiz: bool| -> Vec<f32> {
        let mut out = vec![0f32; w * h];
        let r = LINE_REACH as isize;
        for y in 0..h {
            for x in 0..w {
                let mut s = 0.0;
                for k in -r..=r {
                    let (xx, yy) = if horiz { ((x as isize + k).clamp(0, w as isize - 1) as usize, y) } else { (x, (y as isize + k).clamp(0, h as isize - 1) as usize) };
                    s += n.d[yy * w + xx];
                }
                out[y * w + x] = s / (2 * r + 1) as f32;
            }
        }
        out
    };
    let (hl, vl) = (along(true), along(false));
    let lift = ((2 * LINE_REACH + 1) as f32).sqrt();
    let ink: Vec<u8> = (0..w * h)
        .map(|i| {
            // a rule is dark along itself and light across: a stain is dark
            // both ways
            let step = (LINE_K * grain[i] / lift).max(LINE_MIN);
            let (dh, dv) = (hl[i] < level[i] - step, vl[i] < level[i] - step);
            let rule = (dh != dv) && n.d[i] < level[i] - step;
            // coloured ink (a stamp, a blue pen) is kept however pale: a
            // shadow is grey
            let tint = if img.c.len() == 3 {
                let (r, g, b) = (img.c[0].d[i], img.c[1].d[i], img.c[2].d[i]);
                r.max(g).max(b) - r.min(g).min(b) > TINT
            } else {
                false
            };
            ((n.d[i] < thr[i] && near[i] != 0) || rule || tint) as u8
        })
        .collect();
    drop((near, hl, vl));
    // solid ink, dark all through: joined to the edged ink it touches
    let core: Vec<u8> = n.d.iter().map(|&v| (v < SOLID) as u8).collect();
    let solid = grow(&core, &ink, w, h);
    let mut ink: Vec<u8> = ink.iter().zip(&solid).map(|(&a, &b)| a | b).collect();
    // grain that passed for ink in a deep shadow: specks of a few pixels
    let speck = (w.min(h) / SPECK_DIV).max(2);
    for comp in crate::detect::components(&ink, w, h) {
        if comp.len() <= speck {
            for i in comp {
                ink[i] = 0;
            }
        }
    }
    let share = ink.iter().map(|&v| v as f64).sum::<f64>() / (w * h) as f64 * 100.0;
    let m = morph_k(&ink, w, h, &ops::square(1), true);
    let m = blur(&Plane { w, h, d: m.iter().map(|&v| v as f32).collect() }, 0.8);
    for p in img.c.iter_mut() {
        ops::rows(&mut p.d, w, |y, row| {
            for (x, v) in row.iter_mut().enumerate() {
                let i = y * w + x;
                let t = ((*v - INK_BLACK) / (INK_WHITE - INK_BLACK)).clamp(0.0, 1.0).powf(INK_GAMMA);
                *v = t * m.d[i] + (1.0 - m.d[i]);
            }
        });
    }
    img.q8();
    share
}

/// The bright quantile a block's paper is read at, and how far below the
/// bright blocks around it a block is print.
const BG_QUANTILE: f64 = 0.9;
const BG_PRINT: f32 = 0.7;
/// The paper's grain is averaged this widely; ink is darker than white by
/// GRAIN_K times it, and at least INK_MIN.
const GRAIN_SIGMA: f64 = 17.0;
const GRAIN_K: f32 = 4.0;
const INK_MIN: f32 = 0.08;
/// A Sobel step that counts as an edge, and how far from one ink may lie.
const EDGE: f32 = 0.12;
const EDGE_REACH: usize = 4;
/// Darker than this is ink all through, however far from its edge.
const SOLID: f32 = 0.55;
/// Ink's tone: this dark goes to black, and the curve presses it darker.
const INK_BLACK: f32 = 0.2;
const INK_GAMMA: f32 = 1.5;
const INK_WHITE: f32 = 0.97;
/// A rule is looked for along this many pixels either way, and must be
/// this much darker than white.
const LINE_REACH: usize = 7;
const LINE_MIN: f32 = 0.04;
const LINE_K: f32 = 3.0;
/// A pixel this far from grey is coloured ink, kept whatever its edges.
const TINT: f32 = 0.12;
/// Ink blobs of at most short side / SPECK_DIV pixels are grain.
const SPECK_DIV: usize = 250;

/// Lines of text on a sheet bent in the hand, made straight again. The ink
/// of each line is smeared along it into one long thin blob; the middle of
/// each blob is sampled across its length. Every sample should lie on its
/// line's own straight row, so one smooth field D(x, y), a cubic in x and a
/// quadratic in y with no x-free term, is fitted to all of them at once, and
/// the page is resampled with D taken out. It only runs when there are
/// enough lines over enough of the page to say what the bend is, and the
/// bend it finds is modest; otherwise the page is left as it is. Returns the
/// largest shift, in pixels, and the number of lines it went by.
pub fn straighten(img: &mut Img) -> Option<(f64, usize)> {
    let (w0, h0) = (img.w, img.h);
    // the lines are found on a copy about 1200 px wide
    let k = (w0 as f64 / DW_SIDE).max(1.0);
    let (w, h) = ((w0 as f64 / k).round() as usize, (h0 as f64 / k).round() as usize);
    let g = crate::img::resize_any(img, w, h).gray();
    let ink: Vec<u8> = g.d.iter().map(|&v| (v < DW_INK) as u8).collect();
    let kx = (w / 120).max(7) as usize;
    let rect = |half: usize, rows: isize| -> ops::Kernel { (-rows..=rows).map(|dy| (dy, half)).collect() };
    let c = morph_k(&morph_k(&ink, w, h, &rect(kx, 1), true), w, h, &rect(kx, 1), false);
    let c = morph_k(&morph_k(&c, w, h, &rect(kx / 2, 0), false), w, h, &rect(kx / 2, 0), true);
    let comps = crate::detect::components(&c, w, h);
    let boxes: Vec<(usize, usize, usize, usize, &Vec<usize>)> = comps
        .iter()
        .map(|p| {
            let (mut x0, mut x1, mut y0, mut y1) = (w, 0, h, 0);
            for &i in p {
                let (x, y) = (i % w, i / w);
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
            (x0, x1 - x0 + 1, y0, y1 - y0 + 1, p)
        })
        .filter(|b| b.1 as f64 >= DW_LINE_LEN * w as f64 && b.1 >= 8 * b.3)
        .collect();
    if boxes.len() < DW_MIN_LINES {
        return None;
    }
    let mut hs: Vec<usize> = boxes.iter().map(|b| b.3).collect();
    hs.sort_unstable();
    let hmed = hs[hs.len() / 2];
    // samples: (line, x, y), both in parts of the page
    let mut samples: Vec<(usize, f64, f64)> = vec![];
    let mut lines = 0;
    let (mut ymin, mut ymax) = (1.0f64, 0.0f64);
    for &(x0, bw, y0, bh, pix) in &boxes {
        if bh > 2 * hmed {
            continue;
        }
        let step = (bw / 30).max(4);
        let mut span = vec![(usize::MAX, 0usize); bw.div_ceil(step)];
        for &i in pix {
            let (x, y) = (i % w - x0, i / w);
            let s = &mut span[x / step];
            s.0 = s.0.min(y);
            s.1 = s.1.max(y);
        }
        let before = samples.len();
        for (k, s) in span.iter().enumerate() {
            if s.0 != usize::MAX && (k + 1) * step <= bw {
                let (x, y) = ((x0 + k * step) as f64 + step as f64 / 2.0, (s.0 + s.1) as f64 / 2.0);
                samples.push((lines, x / w as f64, y / h as f64));
            }
        }
        if samples.len() > before {
            ymin = ymin.min(y0 as f64 / h as f64);
            ymax = ymax.max((y0 + bh) as f64 / h as f64);
            lines += 1;
        }
    }
    if lines < DW_MIN_LINES || ymax - ymin < DW_COVER {
        return None;
    }
    // y = D(x, y) + c_line, least squares over the terms and one offset a line
    let terms: Vec<(i32, i32)> = (1..=3).flat_map(|i| (0..=2).map(move |j| (i, j))).collect();
    let n = terms.len() + lines;
    let mut ata = vec![vec![0.0; n]; n];
    let mut atb = vec![0.0; n];
    let row = |l: usize, x: f64, y: f64| -> Vec<(usize, f64)> {
        let mut r: Vec<(usize, f64)> = terms.iter().enumerate().map(|(t, &(i, j))| (t, (x - 0.5).powi(i) * y.powi(j))).collect();
        r.push((terms.len() + l, 1.0));
        r
    };
    for &(l, x, y) in &samples {
        let r = row(l, x, y);
        for &(a, va) in &r {
            atb[a] += va * y;
            for &(b, vb) in &r {
                ata[a][b] += va * vb;
            }
        }
    }
    let sol = crate::img::solve(ata, atb)?;
    let coef: Vec<f64> = sol[..terms.len()].to_vec();
    let rms = (samples.iter().map(|&(l, x, y)| (y - row(l, x, y).iter().map(|&(a, v)| sol[a] * v).sum::<f64>()).powi(2)).sum::<f64>() / samples.len() as f64).sqrt() * h as f64;
    if rms > DW_RMS * hmed as f64 {
        return None;
    }
    let d = |x: f64, y: f64| -> f64 { terms.iter().zip(&coef).map(|(&(i, j), c)| c * (x - 0.5).powi(i) * y.powi(j)).sum() };
    // the output's rows: y_out = y_in - D(x, y_in), solved for y_in
    let (wf, hf) = (w0 as f64, h0 as f64);
    // the shift on a coarse grid, the field being smooth, then read off it
    const G: usize = 16;
    let (gw, gh) = (w0.div_ceil(G) + 1, h0.div_ceil(G) + 1);
    let mut grid = vec![0f32; gw * gh];
    let mut worst: f64 = 0.0;
    for gy in 0..gh {
        for gx in 0..gw {
            let (xn, yn) = ((gx * G) as f64 / wf, (gy * G) as f64 / hf);
            let mut yi = yn;
            for _ in 0..3 {
                yi = yn + d(xn, yi);
            }
            worst = worst.max((yi - yn).abs());
            grid[gy * gw + gx] = ((yi - yn) * hf) as f32;
        }
    }
    if worst > DW_MAX {
        return None;
    }
    let shift = |x: usize, y: usize| -> f32 {
        let (gx, gy) = (x / G, y / G);
        let (fx, fy) = ((x % G) as f32 / G as f32, (y % G) as f32 / G as f32);
        let at = |i: usize, j: usize| grid[j.min(gh - 1) * gw + i.min(gw - 1)];
        (at(gx, gy) * (1.0 - fx) + at(gx + 1, gy) * fx) * (1.0 - fy) + (at(gx, gy + 1) * (1.0 - fx) + at(gx + 1, gy + 1) * fx) * fy
    };
    for p in img.c.iter_mut() {
        let src = p.d.clone();
        ops::rows(&mut p.d, w0, |y, out| {
            for (x, v) in out.iter_mut().enumerate() {
                let sy = y as f32 + shift(x, y);
                if sy < 0.0 || sy > (h0 - 1) as f32 {
                    *v = 1.0;
                    continue;
                }
                let y0 = sy.floor() as usize;
                let y1 = (y0 + 1).min(h0 - 1);
                let f = sy - y0 as f32;
                *v = src[y0 * w0 + x] * (1.0 - f) + src[y1 * w0 + x] * f;
            }
        });
    }
    img.q8();
    Some((worst * hf, lines))
}

/// The copy the lines are found on, its width in pixels.
const DW_SIDE: f64 = 1200.0;
/// Ink, after the cleaning; a line of text at least this share of the page
/// wide and 8 times as long as tall.
const DW_INK: f32 = 0.63;
const DW_LINE_LEN: f64 = 0.12;
/// Enough lines over enough of the page's height, fitted to within this
/// many line heights, and a bend no larger than this share of the page.
const DW_MIN_LINES: usize = 10;
const DW_COVER: f64 = 0.5;
const DW_RMS: f64 = 0.35;
const DW_MAX: f64 = 0.05;
