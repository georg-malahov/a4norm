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

/// A4TIME=1: the time since the last mark, per labelled step (native only).
pub fn mark(label: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::cell::Cell;
        use std::time::Instant;
        thread_local!(static LAST: Cell<Option<Instant>> = const { Cell::new(None) });
        if std::env::var_os("A4TIME").is_some() {
            let now = Instant::now();
            LAST.with(|l| {
                if let Some(t) = l.get() {
                    eprintln!("time {:28} {:7.1} ms", label, (now - t).as_secs_f64() * 1000.0);
                }
                l.set(Some(now));
            });
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = label;
}
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

/// Whether any pixel is set in the (2r+1)^2 square around each pixel.
fn box_any(m: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    // along rows, then down columns, with running counts
    let rows: Vec<u16> = ops::par_map(h, |y| {
        let row = &m[y * w..(y + 1) * w];
        let at = |x: isize| if x >= 0 && (x as usize) < w { row[x as usize] as u16 } else { 0 };
        let mut c: u16 = (0..=r as isize).map(at).sum();
        let mut out = vec![0u16; w];
        for x in 0..w {
            out[x] = c;
            c = c + at(x as isize + r as isize + 1) - at(x as isize - r as isize);
        }
        out
    })
    .concat();
    let mut out = vec![0u8; w * h];
    let mut c = vec![0u16; w];
    for y in 0..=r.min(h - 1) {
        for (a, &v) in c.iter_mut().zip(&rows[y * w..(y + 1) * w]) {
            *a += v;
        }
    }
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = (c[x] > 0) as u8;
        }
        if y + r + 1 < h {
            for x in 0..w {
                c[x] += rows[(y + r + 1) * w + x];
            }
        }
        if y >= r {
            for x in 0..w {
                c[x] -= rows[(y - r) * w + x];
            }
        }
    }
    out
}

/// Rules along rows and columns whose ink has gaps: a pixel is a ridge when
/// it is darker than the page at both sides across the line (a shade's edge
/// is dark on one side only). Along a row, ridges with short gaps between
/// them that run on for a good share of the page, mostly ink already, are
/// made ink along all their length.
fn fill_rules(ink: &mut [u8], n: &Plane, level: &[f32], w: usize, h: usize) {
    let d = RIDGE_OFF;
    let ridge = |horiz: bool| -> Vec<u8> {
        ops::par_map(h, |y| {
            let mut row = vec![0u8; w];
            for x in 0..w {
                let i = y * w + x;
                let v = n.d[i];
                let (a, b) = if horiz {
                    if y < d || y + d >= h {
                        continue;
                    }
                    (n.d[i - d * w], n.d[i + d * w])
                } else {
                    if x < d || x + d >= w {
                        continue;
                    }
                    (n.d[i - d], n.d[i + d])
                };
                row[x] = (v < a.min(b) - RIDGE_STEP && v < level[i] - RIDGE_STEP) as u8;
            }
            row
        })
        .concat()
    };
    let gap = (w.min(h) as f64 * RULE_GAP) as usize;
    let long = (w.min(h) as f64 * RULE_FILL_LONG) as usize;
    // never along a side close to it: that is the sheet's own edge
    let zl = (w.min(h) as f64 * EDGE_LINE_ZONE) as usize;
    // A rule traced from each ridge pixel along its length (a: along, c:
    // across). It may step one pixel across at a time, as a rule on a sheet
    // that is not quite square does, and cross gaps of up to `gap` straight.
    let trace = |ink: &mut [u8], r: &[u8], horiz: bool| {
        let (len, wid) = if horiz { (w, h) } else { (h, w) };
        let at = |a: usize, c: usize| if horiz { c * w + a } else { a * w + c };
        let mut seen = vec![0u8; w * h];
        for c0 in zl..wid.saturating_sub(zl) {
            for a0 in 0..len {
                if r[at(a0, c0)] == 0 || seen[at(a0, c0)] != 0 {
                    continue;
                }
                let mut path = vec![(a0, c0)];
                let (mut c, mut last, mut on, mut inked) = (c0, a0, 1usize, ink[at(a0, c0)] as usize);
                seen[at(a0, c0)] = 1;
                let mut a = a0 + 1;
                while a < len && a - last <= gap {
                    let hit = [c, c.saturating_sub(1), (c + 1).min(wid - 1)].into_iter().find(|&cc| r[at(a, cc)] != 0);
                    if let Some(cc) = hit {
                        c = cc;
                        last = a;
                        on += 1;
                        inked += ink[at(a, c)] as usize;
                        seen[at(a, c)] = 1;
                    }
                    path.push((a, c));
                    a += 1;
                }
                // the path up to its last ridge
                while path.last().is_some_and(|p| p.0 > last) {
                    path.pop();
                }
                let span = last - a0 + 1;
                if span >= long && on as f64 >= RULE_ON * span as f64 && inked as f64 >= RULE_INKED * on as f64 {
                    for (a, c) in path {
                        ink[at(a, c)] = 1;
                    }
                }
            }
        }
    };
    let rh = ridge(true);
    trace(ink, &rh, true);
    drop(rh);
    let rv = ridge(false);
    trace(ink, &rv, false);
}

/// Ink from the page's border: thick dark parts that reach the border (the
/// desk, a shadow, a curled rim) with a margin round them, then whatever
/// still touches the border only a little way in. Text a little in from the
/// edge (a footer, a publisher's line) touches nothing and stays.
fn off_border(ink: &mut [u8], n: &Plane, w: usize, h: usize) {
    let r = (w.min(h) / THICK_DIV).max(3);
    let thick = morph_k(&morph_k(ink, w, h, &ops::disk(r), false), w, h, &ops::disk(r), true);
    let touch = |x0: usize, x1: usize, y0: usize, y1: usize| x0 <= BORDER_TOUCH || y0 <= BORDER_TOUCH || x1 + BORDER_TOUCH >= w - 1 || y1 + BORDER_TOUCH >= h - 1;
    let bbox = |comp: &[usize]| {
        let (mut x0, mut x1, mut y0, mut y1) = (w, 0, h, 0);
        for &i in comp {
            x0 = x0.min(i % w);
            x1 = x1.max(i % w);
            y0 = y0.min(i / w);
            y1 = y1.max(i / w);
        }
        (x0, x1, y0, y1)
    };
    // thick and dark, touching the border or lying along a side near it
    // (a curled rim's shadow runs a little in from the page's own edge)
    let zone = (w.min(h) as f64 * EDGE_ZONE) as usize;
    let mut bg = vec![0u8; w * h];
    for comp in crate::detect::components(&thick, w, h) {
        let (x0, x1, y0, y1) = bbox(&comp);
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        let along_h = bw >= RIM_LONG * bh && bw as f64 >= EDGE_ALONG * w as f64;
        let along_v = bh >= RIM_LONG * bw && bh as f64 >= EDGE_ALONG * h as f64;
        let by_side = (along_h && (y1 <= zone || y0 + zone >= h - 1)) || (along_v && (x1 <= zone || x0 + zone >= w - 1));
        if touch(x0, x1, y0, y1) || by_side {
            for i in comp {
                bg[i] = 1;
            }
        }
    }
    let bg = morph_k(&bg, w, h, &ops::disk(r + 1), true);
    // text printed on that shadow is darker than it, and stays
    for ((v, &b), &g) in ink.iter_mut().zip(&bg).zip(&n.d) {
        if b != 0 && g > EDGE_TEXT {
            *v = 0;
        }
    }
    let depth = (w.min(h) as f64 * BORDER_DEPTH) as usize;
    for comp in crate::detect::components(ink, w, h) {
        let (x0, x1, y0, y1) = bbox(&comp);
        let shallow = (x0 <= BORDER_TOUCH && x1 <= depth)
            || (x1 + BORDER_TOUCH >= w - 1 && x0 + depth >= w - 1)
            || (y0 <= BORDER_TOUCH && y1 <= depth)
            || (y1 + BORDER_TOUCH >= h - 1 && y0 + depth >= h - 1);
        // grey blotches near a side: shade, not print (print is dark after
        // the division, a pencil mark is thin and away from the side)
        let z = (w.min(h) as f64 * GREY_ZONE) as usize;
        let near = y1 + z >= h - 1 || y0 <= z || x0 <= z || x1 + z >= w - 1;
        let grey = near && {
            let mut v: Vec<f32> = comp.iter().map(|&i| n.d[i]).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2] > GREY_BLOT
        };
        // thin pieces along a side close to it: the sheet's own edge line
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        let thin = (w.min(h) as f64 * EDGE_THIN) as usize;
        let zl = (w.min(h) as f64 * EDGE_LINE_ZONE) as usize;
        let edge_line = (bh <= thin && bw >= 3 * bh && (y1 + zl >= h - 1 || y0 <= zl)) || (bw <= thin && bh >= 3 * bw && (x1 + zl >= w - 1 || x0 <= zl));
        if shallow || edge_line {
            for i in comp {
                ink[i] = 0;
            }
        } else if grey {
            // its grey goes; letters inside it, darker, stay
            for i in comp {
                if n.d[i] > EDGE_TEXT {
                    ink[i] = 0;
                }
            }
        }
    }    // a grey band along a side, thick and long, that print runs into (staff
    // lines, a table's rules): the sheet's rim, cut out of what it touches.
    // Thin rules and dark print survive.
    let strip = (w.min(h) as f64 * RIM_STRIP) as usize;
    let long = (w.min(h) as f64 * RIM_BAND_LONG) as usize;
    let thick = RIM_BAND_THICK;
    let rect = |hw: usize, hh: usize| -> ops::Kernel { (-(hh as isize)..=hh as isize).map(|dy| (dy, hw)).collect() };
    for vertical in [true, false] {
        let k = if vertical { rect(thick, long) } else { rect(long, thick) };
        let band = morph_k(&morph_k(ink, w, h, &k, false), w, h, &k, true);
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let by_side = if vertical { x < strip || x + strip >= w } else { y < strip || y + strip >= h };
                if band[i] != 0 && by_side && n.d[i] > EDGE_TEXT {
                    ink[i] = 0;
                }
            }
        }
    }
    // once the shade is gone, the sheet's edge line it hid is on its own
    let thin = (w.min(h) as f64 * EDGE_THIN) as usize;
    let zl = (w.min(h) as f64 * EDGE_LINE_ZONE) as usize;
    for comp in crate::detect::components(ink, w, h) {
        let (x0, x1, y0, y1) = bbox(&comp);
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        if (bh <= thin && bw >= 3 * bh && (y1 + zl >= h - 1 || y0 <= zl)) || (bw <= thin && bh >= 3 * bw && (x1 + zl >= w - 1 || x0 <= zl)) {
            for i in comp {
                ink[i] = 0;
            }
        }
    }
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
    // (all three are smooth: summed over 4x4 cells, blurred there, brought
    // back up)
    let (qw, qh) = (w.div_ceil(GRAIN_CELL), h.div_ceil(GRAIN_CELL));
    // one row of cells per task
    let bands = ops::par_map(qh, |cy| {
        let mut b = [vec![0f32; qw], vec![0f32; qw], vec![0f32; qw]];
        for y in cy * GRAIN_CELL..((cy + 1) * GRAIN_CELL).min(h) {
            for (x, &v) in n.row(y).iter().enumerate() {
                if v > 0.8 {
                    let c = x / GRAIN_CELL;
                    b[0][c] += 1.0 - v;
                    b[1][c] += 1.0;
                    b[2][c] += v;
                }
            }
        }
        b
    });
    let cells: [Vec<f32>; 3] = std::array::from_fn(|k| bands.iter().flat_map(|b| b[k].iter().copied()).collect());
    // as shares of a cell, which the resize keeps within 0..1
    let per = 1.0 / (GRAIN_CELL * GRAIN_CELL) as f32;
    let up = |d: Vec<f32>| ops::resize(&blur(&Plane { w: qw, h: qh, d: d.iter().map(|v| v * per).collect() }, GRAIN_SIGMA / GRAIN_CELL as f64), w, h, Filter::Triangle);
    let [dev, cnt, lit] = cells;
    let (dev, cnt, lit) = (up(dev), up(cnt), up(lit));
    let grain: Vec<f32> = dev.d.iter().zip(&cnt.d).map(|(&d, &c)| 1.25 * d / c.max(1e-3)).collect();
    let thr: Vec<f32> = grain.iter().map(|&g| 1.0 - (GRAIN_K * g).max(INK_MIN)).collect();
    // and its own level there, which a slow shade left after the division
    // keeps a little under white
    let level: Vec<f32> = lit.d.iter().zip(&cnt.d).map(|(&l, &c)| if c > 1e-3 { l / c } else { 1.0 }).collect();
    drop((dev, cnt, lit));
    mark("  ip: grain+level blurs");
    // an edge next to it: Sobel of the slightly blurred page
    let b = blur(&n, 1.0);
    let edge: Vec<u8> = ops::par_map(h, |y| {
        let mut row = vec![0u8; w];
        if y == 0 || y + 1 >= h {
            return row;
        }
        for x in 1..w - 1 {
            let at = |dx: isize, dy: isize| b.d[(y as isize + dy) as usize * w + (x as isize + dx) as usize];
            let gx = at(1, -1) + 2.0 * at(1, 0) + at(1, 1) - at(-1, -1) - 2.0 * at(-1, 0) - at(-1, 1);
            let gy = at(-1, 1) + 2.0 * at(0, 1) + at(1, 1) - at(-1, -1) - 2.0 * at(0, -1) - at(1, -1);
            row[x] = (gx * gx + gy * gy > EDGE * EDGE) as u8;
        }
        row
    })
    .concat();
    drop(b);
    mark("  ip: sobel");
    // near an edge: any edge pixel in the square around it, by running sums
    let near = box_any(&edge, w, h, EDGE_REACH);
    drop(edge);
    mark("  ip: near dilate");
    // a faint rule: averaged along a row or a column the grain goes, the rule
    // stays
    let r = LINE_REACH;
    let along = |horiz: bool| -> Vec<f32> {
        let k = 1.0 / (2 * r + 1) as f32;
        if horiz {
            ops::par_map(h, |y| {
                let row = n.row(y);
                let at = |x: isize| row[x.clamp(0, w as isize - 1) as usize];
                let mut s: f32 = (-(r as isize)..=r as isize).map(at).sum();
                let mut out = vec![0f32; w];
                for x in 0..w {
                    out[x] = s * k;
                    s += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
                }
                out
            })
            .concat()
        } else {
            let mut out = vec![0f32; w * h];
            let at = |y: isize| &n.d[y.clamp(0, h as isize - 1) as usize * w..][..w];
            let mut s = vec![0f32; w];
            for dy in -(r as isize)..=r as isize {
                for (a, &v) in s.iter_mut().zip(at(dy)) {
                    *a += v;
                }
            }
            for y in 0..h {
                for (o, &a) in out[y * w..(y + 1) * w].iter_mut().zip(&s) {
                    *o = a * k;
                }
                let (add, sub) = (at(y as isize + r as isize + 1), at(y as isize - r as isize));
                for x in 0..w {
                    s[x] += add[x] - sub[x];
                }
            }
            out
        }
    };
    let (hl, vl) = (along(true), along(false));
    mark("  ip: along");
    let lift = ((2 * LINE_REACH + 1) as f32).sqrt();
    let mut rules = vec![0u8; w * h];
    let ink: Vec<u8> = (0..w * h)
        .map(|i| {
            // coloured ink (a stamp, a blue pen) is kept however pale: a
            // shadow is grey. Its colour is the ink's hue against the light
            // around it, and it is a mark, with an edge near: a warm or
            // tinted light over a fold is neither
            let tint = img.c.len() == 3 && {
                let (r, g, b) = (img.c[0].d[i], img.c[1].d[i], img.c[2].d[i]);
                let c = r.max(g).max(b) - r.min(g).min(b);
                // strongly coloured (a stamp's pale fill), or coloured, dark
                // enough and marked
                c > 2.0 * TINT || (c > TINT && near[i] != 0 && n.d[i] < level[i] - TINT_DARK)
            };
            ((n.d[i] < thr[i] && near[i] != 0) || tint) as u8
        })
        .collect();
    mark("  ip: ink map");
    // a rule is dark along itself and light across: a stain is dark both ways
    for i in 0..w * h {
        let step = (LINE_K * grain[i] / lift).max(LINE_MIN);
        let (dh, dv) = (hl[i] < level[i] - step, vl[i] < level[i] - step);
        rules[i] = ((dh != dv) && n.d[i] < level[i] - step) as u8;
    }
    // a rule runs on: short bits of it are a shade's edge or the grain
    let long = (w.min(h) as f64 * RULE_LONG) as usize;
    let mut ink = ink;
    for comp in crate::detect::components(&rules, w, h) {
        let (mut x0, mut x1, mut y0, mut y1) = (w, 0, h, 0);
        for &i in &comp {
            x0 = x0.min(i % w);
            x1 = x1.max(i % w);
            y0 = y0.min(i / w);
            y1 = y1.max(i / w);
        }
        if (x1 - x0).max(y1 - y0) >= long {
            for i in comp {
                ink[i] = 1;
            }
        }
    }
    drop((near, hl, vl));
    mark("  ip: rules");
    // solid ink, dark all through: joined to the edged ink it touches
    let core: Vec<u8> = n.d.iter().map(|&v| (v < SOLID) as u8).collect();
    let solid = grow(&core, &ink, w, h);
    let mut ink: Vec<u8> = ink.iter().zip(&solid).map(|(&a, &b)| a | b).collect();
    mark("  ip: solid grow");
    // a long thin rule, faint in places: kept whole
    fill_rules(&mut ink, &n, &level, w, h);
    mark("  ip: rules filled");
    // what comes in from the page's border: the desk, a shadow or the curled
    // rim a rectify left along a side, dark and thick, and whatever touches
    // the border only a little way in
    off_border(&mut ink, &n, w, h);
    mark("  ip: off border");
    // grain that passed for ink in a deep shadow: specks of a few pixels
    let speck = (w.min(h) / SPECK_DIV).max(2);
    // and small coloured crumbs: the back's print showing through a fold,
    // tinted by the light (a colour mark worth keeping is larger)
    let crumb = speck * CRUMB_K;
    let chroma = |i: usize| {
        if img.c.len() == 3 {
            let (r, g, b) = (img.c[0].d[i], img.c[1].d[i], img.c[2].d[i]);
            r.max(g).max(b) - r.min(g).min(b)
        } else {
            0.0
        }
    };
    for comp in crate::detect::components(&ink, w, h) {
        let tinted = comp.len() <= crumb && comp.iter().map(|&i| chroma(i)).sum::<f32>() / comp.len() as f32 > TINT;
        if comp.len() <= speck || tinted {
            for i in comp {
                ink[i] = 0;
            }
        }
    }
    // what lies along the page's own edge, thin across it: the sheet's rim,
    // a curl, a shadow line the rectify left
    let band = (w.min(h) as f64 * EDGE_BAND) as usize;
    for comp in crate::detect::components(&ink, w, h) {
        let (mut x0, mut x1, mut y0, mut y1) = (w, 0, h, 0);
        for &i in &comp {
            x0 = x0.min(i % w);
            x1 = x1.max(i % w);
            y0 = y0.min(i / w);
            y1 = y1.max(i / w);
        }
        // a rim runs along the side: long along it, thin across (a line of
        // small print at the foot is neither)
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        let along_v = bh >= RIM_LONG * bw;
        let along_h = bw >= RIM_LONG * bh;
        let thin_left = along_v && x0 <= band && x1 <= 2 * band;
        let thin_right = along_v && x1 + band >= w - 1 && x0 + 2 * band >= w - 1;
        let thin_top = along_h && y0 <= band && y1 <= 2 * band;
        let thin_bottom = along_h && y1 + band >= h - 1 && y0 + 2 * band >= h - 1;
        if thin_left || thin_right || thin_top || thin_bottom {
            for i in comp {
                ink[i] = 0;
            }
        }
    }
    mark("  ip: specks+rim");
    let share = ink.iter().map(|&v| v as f64).sum::<f64>() / (w * h) as f64 * 100.0;
    let m = morph_k(&ink, w, h, &ops::square(1), true);
    let m = blur(&Plane { w, h, d: m.iter().map(|&v| v as f32).collect() }, 0.8);
    for p in img.c.iter_mut() {
        ops::rows(&mut p.d, w, |y, row| {
            for (x, v) in row.iter_mut().enumerate() {
                let i = y * w + x;
                let t = ((*v - INK_BLACK) / (INK_WHITE - INK_BLACK)).clamp(0.0, 1.0);
                let t = t * t.sqrt();
                *v = t * m.d[i] + (1.0 - m.d[i]);
            }
        });
    }
    img.q8();
    mark("  ip: tone");
    share
}

/// The bright quantile a block's paper is read at, and how far below the
/// bright blocks around it a block is print.
const BG_QUANTILE: f64 = 0.9;
const BG_PRINT: f32 = 0.7;
/// The paper's grain is averaged this widely; ink is darker than white by
/// GRAIN_K times it, and at least INK_MIN.
const GRAIN_SIGMA: f64 = 17.0;
const GRAIN_CELL: usize = 4;
const GRAIN_K: f32 = 4.0;
const INK_MIN: f32 = 0.08;
/// A Sobel step that counts as an edge, and how far from one ink may lie.
const EDGE: f32 = 0.12;
const EDGE_REACH: usize = 4;
/// Darker than this is ink all through, however far from its edge.
const SOLID: f32 = 0.55;
/// Ink's tone: this dark goes to black, and a power of 1.5 presses it darker.
const INK_BLACK: f32 = 0.2;
const INK_WHITE: f32 = 0.97;
/// A rule is looked for along this many pixels either way, and must be
/// this much darker than white.
const LINE_REACH: usize = 7;
const LINE_MIN: f32 = 0.04;
const LINE_K: f32 = 3.0;
/// A pixel this far from grey is coloured ink, kept whatever its edges.
const TINT: f32 = 0.12;
/// and at least this much darker than the paper: the print on the back
/// showing through is pale.
const TINT_DARK: f32 = 0.08;
/// Ink blobs of at most short side / SPECK_DIV pixels are grain.
const SPECK_DIV: usize = 250;
/// A coloured blob up to CRUMB_K specks is a crumb.
const CRUMB_K: usize = 12;
/// A rule is at least this share of the page long.
const RULE_LONG: f64 = 0.05;
/// Ink inside this share of the page from its edge, and no deeper than
/// twice that, is the sheet's rim.
const EDGE_BAND: f64 = 0.015;
const RIM_LONG: usize = 4;
/// A rule's ridge: darker by RIDGE_STEP than the page RIDGE_OFF px to both
/// sides across it. Runs with gaps of at most RULE_GAP of the page, at least
/// RULE_FILL_LONG long, ridge along RULE_ON of it and ink along RULE_INKED of
/// the ridge, are filled.
const RIDGE_OFF: usize = 3;
const RIDGE_STEP: f32 = 0.03;
const RULE_GAP: f64 = 0.01;
const RULE_FILL_LONG: f64 = 0.08;
const RULE_ON: f64 = 0.6;
const RULE_INKED: f64 = 0.3;
/// Thick is what survives an opening of short side / THICK_DIV; a part that
/// touches the border within BORDER_TOUCH px and reaches no more than
/// BORDER_DEPTH of the page in is off the border.
const THICK_DIV: usize = 300;
const BORDER_TOUCH: usize = 2;
const BORDER_DEPTH: f64 = 0.03;
/// A thick band lying along a side within EDGE_ZONE of the page, at least
/// EDGE_ALONG of the side long, is the rim's shadow.
const EDGE_ZONE: f64 = 0.07;
const EDGE_ALONG: f64 = 0.2;
/// Darker than this is print on the shade; a blot within GREY_ZONE of a side
/// whose median is lighter than GREY_BLOT is shade.
const EDGE_TEXT: f32 = 0.4;
const GREY_ZONE: f64 = 0.035;
const GREY_BLOT: f32 = 0.5;
/// A piece no thicker than this share of the page, lying along a side
/// within GREY_ZONE, is the sheet's edge.
const EDGE_THIN: f64 = 0.004;
const EDGE_LINE_ZONE: f64 = 0.06;
/// A rim band: in the outer RIM_STRIP of the page, at least RIM_BAND_LONG of
/// it long and 2*RIM_BAND_THICK+1 px thick.
const RIM_STRIP: f64 = 0.025;
const RIM_BAND_LONG: f64 = 0.03;
const RIM_BAND_THICK: usize = 2;
/// The strip along a side where cut print is looked for.
const EDGE_INK: f64 = 0.02;

/// Lines of text on a sheet bent in the hand, made straight again. The ink
/// of each line is smeared along it into one long thin blob; the middle of
/// each blob is sampled across its length. Every sample should lie on its
/// line's own straight row, so one smooth field D(x, y), a cubic in x and a
/// quadratic in y with no x-free term, is fitted to all of them at once, and
/// the page is resampled with D taken out. It only runs when there are
/// enough lines over enough of the page to say what the bend is, and the
/// bend it finds is modest; otherwise the page is left as it is. Returns the
/// largest shift, in pixels, and the number of lines it went by.
/// The lines of text on a page, sampled along their middles: (line, x, y) in
/// parts of the page, the number of lines, the band of the page they cover,
/// and the median line height and the size of the copy they were read on.
pub struct Lines {
    pub samples: Vec<(usize, f64, f64)>,
    pub lines: usize,
    pub cover: f64,
    pub hmed: usize,
    pub h: usize,
}

pub fn text_lines(img: &Img, min_lines: usize) -> Option<Lines> {
    let (w0, h0) = (img.w, img.h);
    // the lines are found on a copy about 1200 px wide
    let k = (w0 as f64 / DW_SIDE).max(1.0);
    let (w, h) = ((w0 as f64 / k).round() as usize, (h0 as f64 / k).round() as usize);
    let g = crate::img::resize_any(img, w, h).gray();
    // against the paper around it, so a page not yet cleaned reads the same
    // the paper's light, read on a quarter-size copy: it is smooth
    let (qw, qh) = ((w / 4).max(8), (h / 4).max(8));
    let small = ops::resize(&g, qw, qh, Filter::Triangle);
    let bg = Plane { w: qw, h: qh, d: morph_k(&small.d, qw, qh, &ops::square(3), true) };
    let bg = ops::resize(&blur(&bg, 2.0), w, h, Filter::Triangle);
    let ink: Vec<u8> = g.d.iter().zip(&bg.d).map(|(&v, &b)| (v < DW_INK * b.max(0.05)) as u8).collect();
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
    if boxes.len() < min_lines {
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
    Some(Lines { samples, lines, cover: ymax - ymin, hmed, h })
}

/// How far the lines of text are from level, in the upper and the lower half
/// of the page: the median, over the lines there, of each line's spread off
/// its own mean height, in line heights.
pub fn bend(img: &Img) -> Option<[f64; 2]> {
    let l = text_lines(img, 3)?;
    Some(bend_of(&l.samples, l.lines, l.h, l.hmed))
}

/// The same off samples: (line, x, y) in parts of the page, read on a copy
/// `h` tall whose lines are `hmed` high.
fn bend_of(samples: &[(usize, f64, f64)], lines: usize, h: usize, hmed: usize) -> [f64; 2] {
    let mut per: Vec<Vec<f64>> = vec![vec![]; lines];
    for &(i, _, y) in samples {
        per[i].push(y * h as f64);
    }
    let mut halves: [Vec<f64>; 2] = [vec![], vec![]];
    for ys in per.iter().filter(|p| p.len() >= 6) {
        let n = ys.len() as f64;
        let my = ys.iter().sum::<f64>() / n;
        // off level: a line of a flat page runs straight across it, so its
        // tilt counts as much as its bend
        let rms = (ys.iter().map(|y| (y - my).powi(2)).sum::<f64>() / n).sqrt();
        halves[(my / h as f64 >= 0.5) as usize].push(rms / hmed.max(1) as f64);
    }
    let med = |v: &mut Vec<f64>| -> f64 {
        if v.is_empty() {
            return f64::NAN;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    [med(&mut halves[0]), med(&mut halves[1])]
}

/// How much print touches each side of the page (left, right, top, bottom):
/// the share of dark pixels in the outermost EDGE_INK of it, against the
/// paper around them. Print running off a side means that side cut into it.
pub fn edge_ink(img: &Img) -> [f64; 4] {
    let k = (img.w as f64 / 600.0).max(1.0);
    let (w, h) = ((img.w as f64 / k).round() as usize, (img.h as f64 / k).round() as usize);
    let g = crate::img::resize_any(img, w, h).gray();
    let (qw, qh) = ((w / 4).max(8), (h / 4).max(8));
    let small = ops::resize(&g, qw, qh, Filter::Triangle);
    let bg = ops::resize(&blur(&Plane { w: qw, h: qh, d: morph_k(&small.d, qw, qh, &ops::square(2), true) }, 1.5), w, h, Filter::Triangle);
    let dark = |x: usize, y: usize| (g.d[y * w + x] < 0.6 * bg.d[y * w + x]) as u8 as f64;
    let b = ((w.min(h) as f64 * EDGE_INK) as usize).max(2);
    let mut out = [0.0; 4];
    for y in h / 20..h - h / 20 {
        for x in 0..b {
            out[0] += dark(x, y);
            out[1] += dark(w - 1 - x, y);
        }
    }
    for x in w / 20..w - w / 20 {
        for y in 0..b {
            out[2] += dark(x, y);
            out[3] += dark(x, h - 1 - y);
        }
    }
    let (nv, nh) = ((b * (h - h / 10)) as f64, (b * (w - w / 10)) as f64);
    [out[0] / nv, out[1] / nv, out[2] / nh, out[3] / nh]
}

/// What straightening did: the largest shift in pixels and the lines it went
/// by, or, when it would have left one half of the page more bent, how bent
/// the halves were before and would have been after (the page untouched).
pub enum Straight {
    Done(f64, usize),
    Worse([f64; 2], [f64; 2]),
}

pub fn straighten(img: &mut Img, worse: impl Fn(f64, f64) -> bool) -> Option<Straight> {
    let (w0, h0) = (img.w, img.h);
    let Lines { samples, lines, cover, hmed, h } = text_lines(img, DW_MIN_LINES)?;
    if lines < DW_MIN_LINES || cover < DW_COVER {
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
    // judged before any pixel moves: the samples themselves moved by the
    // field, as the page would be
    let before = bend_of(&samples, lines, h, hmed);
    let moved: Vec<(usize, f64, f64)> = samples
        .iter()
        .map(|&(l, x, y)| {
            let mut yo = y;
            for _ in 0..3 {
                yo = y - d(x, yo);
            }
            (l, x, yo)
        })
        .collect();
    let after = bend_of(&moved, lines, h, hmed);
    if (0..2).any(|k| !after[k].is_nan() && !before[k].is_nan() && worse(before[k], after[k])) {
        return Some(Straight::Worse(before, after));
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
    Some(Straight::Done(worst * hf, lines))
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

// ------------------------------------------------------- a sheet's own edges

type Pt = (f64, f64);

/// One side of the sheet: the straight chord between its corners and a bow
/// off it, zero at both corners: e(t) = t(1-t)(a + b(2t-1) + c(2t-1)^2).
struct Side {
    a: Pt,
    b: Pt,
    bow: [f64; 3],
}

impl Side {
    fn normal(&self) -> Pt {
        let (dx, dy) = (self.b.0 - self.a.0, self.b.1 - self.a.1);
        let l = dx.hypot(dy).max(1e-9);
        (-dy / l, dx / l)
    }
    fn at(&self, t: f64) -> Pt {
        let s = 2.0 * t - 1.0;
        let e = t * (1.0 - t) * (self.bow[0] + self.bow[1] * s + self.bow[2] * s * s);
        let n = self.normal();
        (self.a.0 + (self.b.0 - self.a.0) * t + n.0 * e, self.a.1 + (self.b.1 - self.a.1) * t + n.1 * e)
    }
    fn len(&self) -> f64 {
        let mut l = 0.0;
        let mut p = self.at(0.0);
        for k in 1..=32 {
            let q = self.at(k as f64 / 32.0);
            l += (q.0 - p.0).hypot(q.1 - p.1);
            p = q;
        }
        l
    }
}

/// The bow of one side, fitted to the paper's outline points near its chord.
/// A finger or a shadow bites into the paper, so points well inside the fit
/// are dropped and it is fitted again.
fn fit_side(a: Pt, b: Pt, pts: &[Pt], inward: Pt) -> Option<[f64; 3]> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let l = l2.sqrt();
    let n = (-dy / l, dx / l);
    let mut obs: Vec<(f64, f64)> = pts
        .iter()
        .filter_map(|p| {
            let t = ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2;
            let e = (p.0 - a.0) * n.0 + (p.1 - a.1) * n.1;
            ((SIDE_T.0..=SIDE_T.1).contains(&t) && e.abs() < SIDE_BAND * l).then_some((t, e))
        })
        .collect();
    // which way along the normal is into the sheet
    let into = if n.0 * inward.0 + n.1 * inward.1 > 0.0 { 1.0 } else { -1.0 };
    let mut coef = [0.0; 3];
    for _ in 0..4 {
        if obs.len() < SIDE_MIN_PTS {
            return None;
        }
        // the edge must be seen along all of the side, or its bow is a guess
        let mut thirds = [0usize; 3];
        for &(t, _) in &obs {
            thirds[((t * 3.0) as usize).min(2)] += 1;
        }
        if thirds.iter().any(|&c| c < obs.len() / 6) {
            return None;
        }
        let mut m = vec![vec![0.0; 3]; 3];
        let mut r = vec![0.0; 3];
        for &(t, e) in &obs {
            let s = 2.0 * t - 1.0;
            let base = t * (1.0 - t);
            let f = [base, base * s, base * s * s];
            for i in 0..3 {
                r[i] += f[i] * e;
                for j in 0..3 {
                    m[i][j] += f[i] * f[j];
                }
            }
        }
        let c = crate::img::solve(m, r)?;
        coef = [c[0], c[1], c[2]];
        // a sheet's edge bows once, maybe lopsided; a wave is the background
        if coef[2].abs() > coef[0].abs() + SIDE_WAVE * l || coef[1].abs() > 2.0 * coef[0].abs() + SIDE_WAVE * l {
            return None;
        }
        let res: Vec<f64> = obs
            .iter()
            .map(|&(t, e)| {
                let s = 2.0 * t - 1.0;
                (e - t * (1.0 - t) * (coef[0] + coef[1] * s + coef[2] * s * s)) * into
            })
            .collect();
        let mut abs: Vec<f64> = res.iter().map(|v| v.abs()).collect();
        abs.sort_by(|x, y| x.partial_cmp(y).unwrap());
        let sigma = (abs[abs.len() / 2] * 1.5).max(1.0);
        let keep: Vec<(f64, f64)> = obs.iter().zip(&res).filter(|(_, &r)| r < 2.5 * sigma).map(|(o, _)| *o).collect();
        if keep.len() == obs.len() {
            break;
        }
        obs = keep;
    }
    Some(coef)
}

/// A sheet's four bowed edges (top, right, bottom, left), their largest bow
/// in photo pixels, and the flat sheet's size they imply.
pub struct Edges {
    sides: Vec<Side>,
    pub bow: f64,
    pub size: (f64, f64),
}

impl Edges {
    /// The same corners with straight sides: the plain quad, to judge by.
    pub fn straight(&self) -> Edges {
        Edges { sides: self.sides.iter().map(|s| Side { a: s.a, b: s.b, bow: [0.0; 3] }).collect(), bow: 0.0, size: self.size }
    }
}

/// A sheet whose edges bow (held in the hand, curling off the table) mapped
/// flat by its four edge curves, a Coons patch: every straight line of print
/// that ran parallel to an edge comes out straight. The quad gives the
/// corners, the paper's outline the bow of each side. None when no side bows
/// by more than a hair, or the outline cannot be read.
pub fn sheet_edges(src: &crate::img::Src, quad: [Pt; 4]) -> Option<Edges> {
    let vc = crate::detect::vc(src, CURVE_SIDE);
    let pm = crate::detect::paper_mask(&vc, crate::detect::Mode::Paper);
    let (w, h) = (pm.w, pm.h);
    let comps = crate::detect::components(&pm.mask, w, h);
    let best = comps.first()?;
    let mut inside = vec![0u8; w * h];
    for &i in best {
        inside[i] = 1;
    }
    let (sx, sy) = (w as f64 / pm.w0 as f64, h as f64 / pm.h0 as f64);
    let q: Vec<Pt> = quad.iter().map(|p| (p.0 * sx, p.1 * sy)).collect();
    let c = (q.iter().map(|p| p.0).sum::<f64>() / 4.0, q.iter().map(|p| p.1).sum::<f64>() / 4.0);
    // sides: top tl->tr, right tr->br, bottom bl->br, left tl->bl
    let pairs = [(0, 1), (1, 2), (3, 2), (0, 3)];
    let mut sides = vec![];
    let mut worst: f64 = 0.0;
    for &(i, j) in &pairs {
        let (a, b) = (q[i], q[j]);
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        // the outer edge only: along each normal, from outside in, the first
        // paper pixel (the outlines of the print's holes are not the edge)
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx.hypot(dy);
        let mut n = (-dy / l, dx / l);
        if n.0 * (c.0 - mid.0) + n.1 * (c.1 - mid.1) < 0.0 {
            n = (-n.0, -n.1);
        }
        let band = SIDE_BAND * l;
        let mut rim = vec![];
        let steps = (l as usize).max(10);
        for k in 0..steps {
            let t = k as f64 / steps as f64;
            let base = (a.0 + dx * t, a.1 + dy * t);
            let mut e = -band;
            while e < band {
                let (x, y) = (base.0 + n.0 * e, base.1 + n.1 * e);
                if x >= 0.0 && y >= 0.0 && (x as usize) < w && (y as usize) < h && inside[y as usize * w + x as usize] != 0 {
                    rim.push((x, y));
                    break;
                }
                e += 0.5;
            }
        }
        let bow = fit_side(a, b, &rim, (c.0 - mid.0, c.1 - mid.1)).unwrap_or([0.0; 3]);
        let side = Side { a: (a.0 / sx, a.1 / sy), b: (b.0 / sx, b.1 / sy), bow: bow.map(|v| v / sx) };
        for k in 1..16 {
            let t = k as f64 / 16.0;
            let s = 2.0 * t - 1.0;
            worst = worst.max((t * (1.0 - t) * (bow[0] + bow[1] * s + bow[2] * s * s)).abs() / sx);
        }
        sides.push(side);
    }
    let diag = ((q[0].0 - q[2].0).hypot(q[0].1 - q[2].1) / sx).max(1.0);
    if worst < CURVE_MIN * diag || worst > CURVE_MAX * diag {
        return None;
    }
    let ow = ((sides[0].len() + sides[2].len()) / 2.0).round().max(50.0);
    let oh = ((sides[1].len() + sides[3].len()) / 2.0).round().max(50.0);
    Some(Edges { sides, bow: worst, size: (ow, oh) })
}

/// The sheet mapped flat by its edges into a `w`x`h` image: one resampling
/// from the photo, bilinear.
pub fn warp_edges(src: &crate::img::Src, e: &Edges, ow: usize, oh: usize) -> Img {
    use crate::img::Pix;
    let sides = &e.sides;
    let corners = [sides[0].a, sides[0].b, sides[2].b, sides[2].a];
    let nc = src.nc();
    let (sw, sh) = src.dims();
    let mut img = crate::img::warp(ow, oh, nc, |j, row: &mut [&mut [f32]]| {
        let v = (j as f64 + 0.5) / oh as f64;
        let (l, r) = (sides[3].at(v), sides[1].at(v));
        let bil = |u: f64, k: usize| {
            let g = |c: Pt| if k == 0 { c.0 } else { c.1 };
            (1.0 - u) * (1.0 - v) * g(corners[0]) + u * (1.0 - v) * g(corners[1]) + u * v * g(corners[2]) + (1.0 - u) * v * g(corners[3])
        };
        for i in 0..ow {
            let u = (i as f64 + 0.5) / ow as f64;
            let (t, b) = (sides[0].at(u), sides[2].at(u));
            let x = (1.0 - v) * t.0 + v * b.0 + (1.0 - u) * l.0 + u * r.0 - bil(u, 0);
            let y = (1.0 - v) * t.1 + v * b.1 + (1.0 - u) * l.1 + u * r.1 - bil(u, 1);
            let (x, y) = ((x - 0.5).clamp(0.0, sw as f64 - 1.001), (y - 0.5).clamp(0.0, sh as f64 - 1.001));
            let (x0, y0) = (x.floor() as usize, y.floor() as usize);
            let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
            for (k, p) in row.iter_mut().enumerate() {
                let at = |xx: usize, yy: usize| src.at(k, yy * sw + xx);
                p[i] = (at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx) * (1.0 - fy) + (at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx) * fy;
            }
        }
    });
    img.q8();
    img
}

/// The outline is read at this size; a side's points lie within SIDE_BAND of
/// its length off the chord, away from the corners.
const CURVE_SIDE: usize = 800;
const SIDE_BAND: f64 = 0.08;
const SIDE_T: (f64, f64) = (0.04, 0.96);
const SIDE_MIN_PTS: usize = 40;
/// Higher bow terms larger than this share of the side, beyond the plain
/// bow, are not a sheet's edge.
const SIDE_WAVE: f64 = 0.02;
/// A bow worth mapping out, and one too large to be the sheet's own edge,
/// as shares of the sheet's diagonal.
const CURVE_MIN: f64 = 0.004;
const CURVE_MAX: f64 = 0.08;
