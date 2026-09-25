//! One raster -> one A4 page: the script's "rectify", "cards", "the photo
//! passthrough", "finishing" and "the page pipeline" sections, in order.

use crate::detect::{self as d, pc0, Pt, Quad};
use crate::finish;
use crate::img::{self, Img, Pix, Src};
use crate::ops::{self, py_round, Filter, Plane};
use crate::{Fail, Opts};
use std::collections::VecDeque;

pub const MM: f64 = 25.4;

fn dist(a: Pt, b: Pt) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// A portrait A4 page in pixels at `dpi`.
pub fn a4_px(dpi: f64) -> (usize, usize) {
    (py_round(210.0 / MM * dpi) as usize, py_round(297.0 / MM * dpi) as usize)
}

// ------------------------------------ rectify, and what leans in from outside

/// Warp the quad onto a rectangle whose sides average the quad's own.
pub fn rectify(src: &dyn Pix, quad: &Quad, inset_pct: f64, cap: Option<(usize, usize)>, cap_turns: bool, size: Option<(usize, usize)>) -> (Img, usize, usize) {
    let [tl, tr, br, bl] = *quad;
    let mut ww = 50.max(py_round((dist(tl, tr) + dist(bl, br)) / 2.0)) as usize;
    let mut hh = 50.max(py_round((dist(tl, bl) + dist(tr, br)) / 2.0)) as usize;
    if let Some(s) = size {
        (ww, hh) = s;
    } else if let Some(mut cap) = cap {
        if cap_turns && (ww > hh) != (cap.0 > cap.1) {
            cap = (cap.1, cap.0);
        }
        let k = 1f64.min(cap.0 as f64 / ww as f64).min(cap.1 as f64 / hh as f64);
        if k < 1.0 {
            ww = 50.max(py_round(ww as f64 * k)) as usize;
            hh = 50.max(py_round(hh as f64 * k)) as usize;
        }
    }
    let cx = quad.iter().map(|p| p.0).sum::<f64>() / 4.0;
    let cy = quad.iter().map(|p| p.1).sum::<f64>() / 4.0;
    let f = inset_pct / 100.0;
    // the control points as printed with one decimal into the command line
    let r1 = |v: f64| format!("{:.1}", v).parse::<f64>().unwrap();
    let q = quad.map(|p| (r1(p.0 + (cx - p.0) * f), r1(p.1 + (cy - p.1) * f)));
    let to = [(0.0, 0.0), (ww as f64, 0.0), (ww as f64, hh as f64), (0.0, hh as f64)];
    let mut out = img::perspective(src, q, to, ww, hh);
    out.q8();
    (out, ww, hh)
}

const BAND_MARK_DISK: f64 = 0.35;
const BAND_MARK_DELTA: f64 = 35.0;
const BAND_MARK_BED: f64 = 55.0;

/// Per-cell density (0-255) of printed marks, on the flood's own grid.
fn marks_map(img: &Img, w: usize, h: usize, paper: u8) -> Vec<u8> {
    let disk = 3.max(py_round(img.w.min(img.h) as f64 * BAND_MARK_DISK / 100.0) as usize);
    let bed = (paper as f64 * BAND_MARK_BED / 255.0 * 10.0).round() / 10.0;
    let d_it = 1.max((disk * 5 + 7) / 14);
    let s_it = disk.saturating_sub(2 * d_it);
    let g = img.gray();
    let mut c = ops::morph_p(&g, &ops::diamond(d_it), true, 2);
    if s_it > 0 {
        c = ops::morph_p(&c, &ops::square(s_it), true, 1);
        c = ops::morph_p(&c, &ops::square(s_it), false, 1);
    }
    let c = ops::morph_p(&c, &ops::diamond(d_it), false, 2);
    let (dl, bd) = (ops::pct_thr(BAND_MARK_DELTA), ops::pct_thr(bed));
    let m: Vec<u8> = g.d.iter().zip(&c.d).map(|(&a, &b)| ((a - b).abs() > dl && b > bd) as u8).collect();
    let (gw, gh) = (g.w, g.h);
    drop((g, c));
    let m = ops::morph(&m, gw, gh, 1, false);
    let m = ops::morph(&m, gw, gh, 1, true);
    let row = |y: usize, out: &mut [f32]| {
        for (o, &v) in out.iter_mut().zip(&m[y * gw..(y + 1) * gw]) {
            *o = v as f32;
        }
    };
    match ops::default_filter(gw, gh, w, h) {
        Some(f) => ops::resize_rows(gw, gh, &row, w, h, f).bytes(),
        None => m.iter().map(|&v| v * 255).collect(),
    }
}

pub const SIDES: [&str; 4] = ["left", "right", "top", "bottom"];
const RING_MIN: usize = 6;
const RING_COVER: (f64, f64) = (25.0, 75.0);
const RING_SPREAD: f64 = 0.35;

/// Does this side's flood look like a row of binding rings?
fn is_binding(prof: &[usize]) -> bool {
    let peak = match prof.iter().max() {
        Some(&p) if p > 0 => p as f64,
        _ => return false,
    };
    let thr = 1f64.max(peak * 0.2);
    let binv: Vec<u8> = prof.iter().map(|&c| (c as f64 >= thr) as u8).collect();
    let cover = 100.0 * binv.iter().map(|&b| b as f64).sum::<f64>() / binv.len() as f64;
    if !(RING_COVER.0..=RING_COVER.1).contains(&cover) {
        return false;
    }
    let (mut runs, mut gaps, mut cur, mut prev) = (vec![], vec![], 0usize, 0u8);
    for &b in &binv {
        if b != prev {
            if prev != 0 {
                runs.push(cur);
            } else {
                gaps.push(cur);
            }
            cur = 0;
        }
        cur += 1;
        prev = b;
    }
    if prev != 0 {
        runs.push(cur);
    } else {
        gaps.push(cur);
    }
    if gaps.len() > 2 {
        gaps = gaps[1..gaps.len() - 1].to_vec();
    }
    if runs.len() < RING_MIN || gaps.len() < RING_MIN - 1 {
        return false;
    }
    let spread = |v: &[usize]| {
        let m = v.iter().sum::<usize>() as f64 / v.len() as f64;
        if m <= 0.0 {
            return 99.9;
        }
        (v.iter().map(|&x| (x as f64 - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt() / m
    };
    spread(&runs) <= RING_SPREAD && spread(&gaps) <= RING_SPREAD
}

#[derive(Clone, Copy, Default)]
pub struct Verdict {
    pub st: f64,
    pub dk: f64,
    pub doc: bool,
    pub cnt: usize,
}

pub struct Border {
    /// whether the page was repainted
    pub done: bool,
    pub share: f64,
    pub cut: [usize; 4],
    pub verdict: Option<[Verdict; 4]>,
}

fn median(v: &mut [u8]) -> u8 {
    if v.is_empty() {
        return 255;
    }
    v.sort_unstable();
    v[v.len() / 2]
}

/// Lay a flat tone over `img`, in place, through a mask at the analysis
/// scale, grown by `disk`, brought up to size and softened: the repaint of
/// the border flood and of a finger. `tone(k, x, y)` is the colour there.
fn repaint(img: &mut Img, tone: &(dyn Fn(usize, usize, usize) -> f32 + Sync), mask: &[u8], w: usize, h: usize, disk: usize) {
    let m = Plane { w, h, d: mask.iter().map(|&v| v as f32).collect() };
    let m = ops::morph_p(&m, &ops::disk(disk), true, 1);
    let m = ops::blur(&ops::resize_auto(&m, img.w, img.h), 2.0);
    if img.c.len() == 1 {
        *img = img.rgb();
    }
    for (k, p) in img.c.iter_mut().enumerate() {
        let ww = p.w;
        ops::rows(&mut p.d, ww, |y, row| {
            for x in 0..ww {
                let a = m.d[y * ww + x];
                row[x] = tone(k, x, y) * a + row[x] * (1.0 - a);
            }
        });
    }
    img.q8();
}

/// Erase whatever leans in from outside the sheet, without cropping.
pub fn clean_border(img: &mut Img, band_pct: f64, keep_pct: f64, struct_pct: f64, dark_pct: f64) -> Border {
    let max_share = 25.0;
    let side = 520;
    let (w0, h0) = (img.w, img.h);
    let w = side;
    let h = 1.max(py_round(side as f64 * h0 as f64 / w0 as f64)) as usize;
    let (v, c, buf) = d::raw_rgb(img, w, h);
    let n = w * h;
    let mut sv = v.clone();
    sv.sort_unstable();
    let paper = sv[(0.90 * (n - 1) as f64) as usize];
    let v_bad = (paper as f64 * 0.88) as u8;
    let depth = 3.max((w.min(h) as f64 * band_pct / 100.0) as usize);
    let bad = |i: usize| v[i] < v_bad || c[i] > 45;
    let mut reach = vec![0u8; n];
    let mut q = VecDeque::new();
    for x in 0..w {
        for y in [0, h - 1] {
            let i = y * w + x;
            if bad(i) && reach[i] == 0 {
                reach[i] = 1;
                q.push_back((i, 0usize));
            }
        }
    }
    for y in 0..h {
        for x in [0, w - 1] {
            let i = y * w + x;
            if bad(i) && reach[i] == 0 {
                reach[i] = 1;
                q.push_back((i, 0));
            }
        }
    }
    let jump = 2isize;
    while let Some((i, dd)) = q.pop_front() {
        if dd >= depth {
            continue;
        }
        let (x, y) = ((i % w) as isize, (i / w) as isize);
        for dy in -jump..=jump {
            for dx in -jump..=jump {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (xx, yy) = (x + dx, y + dy);
                if xx >= 0 && yy >= 0 && xx < w as isize && yy < h as isize {
                    let j = yy as usize * w + xx as usize;
                    if reach[j] == 0 && bad(j) {
                        reach[j] = 1;
                        q.push_back((j, dd + 1));
                    }
                }
            }
        }
    }
    if !reach.iter().any(|&r| r != 0) {
        return Border { done: false, share: 0.0, cut: [0; 4], verdict: None };
    }
    let marks = marks_map(img, w, h, paper);
    let mut tot = [[0u64; 3]; 4];
    let mut prof: Vec<Vec<usize>> = (0..4).map(|k| vec![0; if k < 2 { h } else { w }]).collect();
    let mut owner = vec![0u8; n];
    let dists = |x: usize, y: usize| [x, w - 1 - x, y, h - 1 - y];
    for i in 0..n {
        if reach[i] == 0 {
            continue;
        }
        let (x, y) = (i % w, i / w);
        let ds = dists(x, y);
        let mn = *ds.iter().min().unwrap();
        let k = ds.iter().position(|&v| v == mn).unwrap();
        owner[i] = k as u8 + 1;
        tot[k][0] += marks[i] as u64;
        tot[k][1] += 1;
        tot[k][2] += v[i] as u64;
        prof[k][if k < 2 { y } else { x }] += 1;
    }
    let mut verdict = [Verdict::default(); 4];
    for k in 0..4 {
        let [mk, cnt, vs] = tot[k];
        let st = if cnt > 0 { 100.0 * mk as f64 / (255.0 * cnt as f64) } else { 0.0 };
        let dk = if cnt > 0 && paper > 0 { 100.0 * vs as f64 / (cnt as f64 * paper as f64) } else { 0.0 };
        let mut doc = st > struct_pct || dk > dark_pct;
        if doc && dk <= dark_pct && is_binding(&prof[k]) {
            doc = false;
        }
        verdict[k] = Verdict { st, dk, doc, cnt: cnt as usize };
    }
    let keep = 2.max((w.min(h) as f64 * keep_pct / 100.0) as usize);
    for i in 0..n {
        let k = owner[i] as usize;
        if k == 0 || !verdict[k - 1].doc {
            continue;
        }
        if dists(i % w, i / w)[k - 1] > keep {
            reach[i] = 0;
        }
    }
    let share = 100.0 * reach.iter().map(|&r| r as f64).sum::<f64>() / n as f64;
    if share <= 0.0 {
        return Border { done: false, share: 0.0, cut: [0; 4], verdict: Some(verdict) };
    }
    if share > max_share {
        return Border { done: false, share, cut: [0; 4], verdict: Some(verdict) };
    }
    let cov_need = 0.15;
    let mut cut = [0usize; 4];
    let mut band: [Option<usize>; 4] = [None; 4];
    for k in 0..4 {
        if verdict[k].doc {
            continue;
        }
        let mut last = 0;
        if k < 2 {
            for kk in 0..depth.min(w) {
                let x = if k == 0 { kk } else { w - 1 - kk };
                let s: usize = (0..h).map(|y| reach[y * w + x] as usize).sum();
                if s as f64 / h as f64 > cov_need {
                    last = kk + 1;
                }
            }
            cut[k] = py_round(last as f64 * w0 as f64 / w as f64) as usize;
        } else {
            for kk in 0..depth.min(h) {
                let y = if k == 2 { kk } else { h - 1 - kk };
                let s: usize = (0..w).map(|x| reach[y * w + x] as usize).sum();
                if s as f64 / w as f64 > cov_need {
                    last = kk + 1;
                }
            }
            cut[k] = py_round(last as f64 * h0 as f64 / h as f64) as usize;
        }
        band[k] = Some(last);
    }
    for i in 0..n {
        let k = owner[i] as usize;
        if k == 0 || reach[i] == 0 {
            continue;
        }
        if let Some(b) = band[k - 1] {
            if dists(i % w, i / w)[k - 1] > b + 2 {
                reach[i] = 0;
            }
        }
    }
    let (mut rs, mut gs, mut bs) = (vec![], vec![], vec![]);
    for i in (0..n).step_by(3) {
        if reach[i] == 0 && v[i] as f64 >= 0.6 * paper as f64 && c[i] <= 45 {
            rs.push(buf[3 * i]);
            gs.push(buf[3 * i + 1]);
            bs.push(buf[3 * i + 2]);
        }
    }
    let tone = [median(&mut rs), median(&mut gs), median(&mut bs)].map(|x| x as f32 / 255.0);
    repaint(img, &|k, _, _| tone[k], &reach, w, h, 3);
    Border { done: true, share, cut, verdict: Some(verdict) }
}

const FINGER_CHROMA: u8 = 45;
const FINGER_RED: i32 = 30;
const FINGER_DEPTH: f64 = 15.0;
const FINGER_OVER: u8 = 25;
const FINGER_STD: u8 = 12;
pub const FINGER_MAX: f64 = 6.0;
const SPINE_BAND: f64 = 8.0;

/// Repaint a finger holding the booklet open, in its page's own tone.
/// `axis`: Some(true) pages side by side, Some(false) stacked, None a card.
/// Returns whether a finger was repainted, in place, and its share.
pub fn erase_fingers(img: &mut Img, axis: Option<bool>) -> (bool, f64) {
    let side = 520;
    let (w0, h0) = (img.w, img.h);
    let w = side;
    let h = 1.max(py_round(side as f64 * h0 as f64 / w0 as f64)) as usize;
    let (v, c, buf) = d::raw_rgb(img, w, h);
    let n = w * h;
    let mut sc = c.clone();
    sc.sort_unstable();
    let c_min = FINGER_CHROMA.max(sc[n / 2].saturating_add(FINGER_OVER));
    let mut skin = vec![0u8; n];
    for i in 0..n {
        let (r, g, b) = (buf[3 * i] as i32, buf[3 * i + 1] as i32, buf[3 * i + 2] as i32);
        if r == v[i] as i32 && c[i] > c_min && r - g.max(b) > FINGER_RED {
            skin[i] = 1;
        }
    }
    let mut sv = v.clone();
    sv.sort_unstable();
    let paper = sv[(0.90 * (n - 1) as f64) as usize] as f64;
    for i in 0..n {
        if skin[i] != 0 && (v[i] as f64) < 0.55 * paper {
            skin[i] = 0;
        }
    }
    let (wf, hf) = (w as f64, h as f64);
    let spine = |x: usize, y: usize| match axis {
        Some(true) => (x as f64 - wf / 2.0).abs() < wf * SPINE_BAND / 200.0,
        Some(false) => (y as f64 - hf / 2.0).abs() < hf * SPINE_BAND / 200.0,
        None => false,
    };
    let depth = 3.max((w.min(h) as f64 * FINGER_DEPTH / 100.0) as usize);
    let mut reach = vec![0u8; n];
    let mut q = VecDeque::new();
    for x in 0..w {
        for y in [0, h - 1] {
            let i = y * w + x;
            if skin[i] != 0 && !spine(x, y) && reach[i] == 0 {
                reach[i] = 1;
                q.push_back((i, 0usize));
            }
        }
    }
    for y in 0..h {
        for x in [0, w - 1] {
            let i = y * w + x;
            if skin[i] != 0 && !spine(x, y) && reach[i] == 0 {
                reach[i] = 1;
                q.push_back((i, 0));
            }
        }
    }
    while let Some((i, dd)) = q.pop_front() {
        if dd >= depth {
            continue;
        }
        let (x, y) = ((i % w) as isize, (i / w) as isize);
        for dx in -1..=1isize {
            for dy in -1..=1isize {
                let (xx, yy) = (x + dx, y + dy);
                if xx >= 0 && yy >= 0 && xx < w as isize && yy < h as isize {
                    let j = yy as usize * w + xx as usize;
                    if skin[j] != 0 && reach[j] == 0 && !spine(xx as usize, yy as usize) {
                        reach[j] = 1;
                        q.push_back((j, dd + 1));
                    }
                }
            }
        }
    }
    if reach.iter().any(|&r| r != 0) {
        let g = img.resize_auto(w, h).gray();
        let sd = ops::stddev(&g, 5).bytes();
        for comp in d::components(&reach, w, h) {
            let mut vals: Vec<u8> = comp.iter().map(|&i| sd[i]).collect();
            vals.sort_unstable();
            if vals[vals.len() / 2] > FINGER_STD {
                for &i in &comp {
                    reach[i] = 0;
                }
            }
        }
    }
    let share = 100.0 * reach.iter().map(|&r| r as f64).sum::<f64>() / n as f64;
    if !(0.05..=FINGER_MAX).contains(&share) {
        return (false, share);
    }
    let parts = if axis.is_some() { 2 } else { 1 };
    let mut tones = vec![];
    for k in 0..parts {
        let (mut rs, mut gs, mut bs) = (vec![], vec![], vec![]);
        for y in 0..h {
            for x in 0..w {
                let inside = match axis {
                    None => true,
                    Some(true) => (x as f64) < wf / 2.0,
                    Some(false) => (y as f64) < hf / 2.0,
                };
                let i = y * w + x;
                if inside == (k == 0) && reach[i] == 0 && v[i] as f64 >= 0.8 * paper {
                    rs.push(buf[3 * i]);
                    gs.push(buf[3 * i + 1]);
                    bs.push(buf[3 * i + 2]);
                }
            }
        }
        tones.push([median(&mut rs), median(&mut gs), median(&mut bs)].map(|x| x as f32 / 255.0));
    }
    // each half of a spread in its own tone, split where +append/-append
    // of the two halves would put the seam
    let tone = |k: usize, x: usize, y: usize| match axis {
        None => tones[0][k],
        Some(true) => tones[(x >= w0 / 2) as usize][k],
        Some(false) => tones[(y >= h0 / 2) as usize][k],
    };
    repaint(img, &tone, &reach, w, h, 2);
    (true, share)
}

// -------------------------------------------- cards: which side, and one page

/// Where a card's face photo is: "left", "right", or None (a back).
fn card_face(img: &Img) -> Option<&'static str> {
    let cells = d::dark_cells(img, 200)?;
    let (dl, rl) = d::place(&cells, 0.03, 0.40, 0.10, 0.95);
    let (dr, rr) = d::place(&cells, 0.60, 0.97, 0.10, 0.95);
    let mut left = dl >= d::CARD_FACE_DARK && rl >= d::CARD_FACE_RUN;
    let mut right = dr >= d::CARD_FACE_DARK && rr >= d::CARD_FACE_RUN;
    if !(left || right) {
        if dl >= d::SPREAD_PHOTO_DARK && dl >= 3.0 * dr + 0.02 {
            left = true;
        } else if dr >= d::SPREAD_PHOTO_DARK && dr >= 3.0 * dl + 0.02 {
            right = true;
        }
    }
    if left && right {
        return Some(if (dl + rl) - (dr + rr) > -0.1 { "left" } else { "right" });
    }
    if left {
        Some("left")
    } else if right {
        Some("right")
    } else {
        None
    }
}

/// A colour copy: light evened by a very wide blur of the brightness, the
/// same factor on every channel, then a gentle stretch (copy_tone_args).
pub fn copy_tone(img: &Img, width: usize) -> Img {
    let sig = py_round(width as f64 / 5.0) as f64;
    let mut mean = img.mean();
    if mean == 0.0 {
        mean = 0.5;
    }
    // "%[fx:mean]" prints six digits, the script passes four
    let mean: f32 = format!("{:.4}", format!("{:.6}", mean).parse::<f64>().unwrap()).parse().unwrap();
    let bg = ops::blur(&img.gray(), sig);
    let mut out = img.each(|p| p.map(|v| (v * mean).min(1.0)).zip(&bg, d::divide));
    out.contrast_stretch(0.3, 0.3);
    out
}

pub struct Card {
    pub img: Img,
    pub front: bool,
}

/// Each card in the frame, rectified to its real size and turned upright.
pub fn process_cards(src: &Src, quads: &[Quad], why: &str, o: &Opts, report: &mut Vec<String>) -> Vec<Card> {
    let dpi = o.dpi as f64;
    let cw_px = py_round(d::CARD_MM.0 / MM * dpi) as usize;
    let ch_px = py_round(d::CARD_MM.1 / MM * dpi) as usize;
    let radius = d::CARD_RADIUS_MM / MM * dpi;
    // INTERFACE: the malahov.io page matches /^card: 1 ID-1 card / (see
    // the script's process_cards)
    report.push(format!(
        "card: {} ID-1 card{} in the frame ({})",
        quads.len(),
        if quads.len() > 1 { "s" } else { "" },
        why
    ));
    let mut cards = vec![];
    for (k, q) in quads.iter().enumerate() {
        let wide = dist(q[0], q[1]) + dist(q[3], q[2]) >= dist(q[0], q[3]) + dist(q[1], q[2]);
        let size = if wide { (cw_px, ch_px) } else { (ch_px, cw_px) };
        let (mut raw, _, _) = rectify(src, q, 0.0, None, false, Some(size));
        let mut turn = if wide { 0 } else { 90 };
        if turn != 0 {
            raw = raw.rotate(90);
        }
        let (got, share) = erase_fingers(&mut raw, None);
        if got {
            report.push(format!("card {}: erased a finger at the edge ({:.1}% of the card)", k + 1, share));
        }
        let side = card_face(&raw);
        if side == Some("right") {
            turn = (turn + 180) % 360;
            raw = raw.rotate(180);
        }
        let front = side.is_some();
        let toned = copy_tone(&raw, cw_px);
        let mut sharp = toned.each(|p| ops::unsharp(p, 1.0, o.sharpen as f32, 0.02));
        // rounded ID-1 corners on white, and a gray70 hairline
        let alpha = img::round_rect(cw_px, ch_px, radius, 0.0);
        let mut card = Img::solid(cw_px, ch_px, [1.0; 3]);
        sharp = sharp.rgb();
        card.over(&sharp, Some(&alpha), 0, 0);
        let sw = 1.max(py_round(dpi / 150.0)) as f64;
        let stroke = img::round_rect(cw_px, ch_px, radius, sw);
        card.over(&Img::solid(cw_px, ch_px, [0.7; 3]), Some(&stroke), 0, 0);
        card.q8();
        report.push(format!(
            "card {}: rectified to {}x{} ({:.1}x{:.2} mm), turned {}°, {}",
            k + 1,
            cw_px,
            ch_px,
            d::CARD_MM.0,
            d::CARD_MM.1,
            turn,
            if front { "face photo on the left — front" } else { "no face photo — back" }
        ));
        cards.push(Card { img: card, front });
    }
    cards
}

/// One A4 with a card's front above its back, at their real size.
pub fn card_page(cards: &[&Card], o: &Opts) -> Img {
    let dpi = o.dpi as f64;
    let pw = py_round(210.0 / MM * dpi) as usize;
    let ph = py_round(297.0 / MM * dpi) as usize;
    let gap = py_round(15.0 / MM * dpi) as usize;
    let (cw_px, ch_px) = if o.card_size == "fit" {
        let cw = py_round(180.0 / MM * dpi) as usize;
        (cw, py_round(cw as f64 * d::CARD_MM.1 / d::CARD_MM.0) as usize)
    } else {
        (py_round(d::CARD_MM.0 / MM * dpi) as usize, py_round(d::CARD_MM.1 / MM * dpi) as usize)
    };
    let total = cards.len() * ch_px + (cards.len() - 1) * gap;
    let mut y = (ph as isize - total as isize).div_euclid(2);
    let x = (pw as isize - cw_px as isize).div_euclid(2);
    let mut page = Img::solid(pw, ph, [1.0; 3]);
    for c in cards {
        let r = c.img.resize_auto(cw_px, ch_px);
        page.over(&r, None, x, y);
        y += (ch_px + gap) as isize;
    }
    page.q8();
    page
}

// ------------------------------------------------------ the photo passthrough

pub struct PageOut {
    pub img: Img,
    pub photo: bool,
    pub dpi: usize,
}

/// The photo path: geometry, and nothing else.
pub fn fit_photo_page(src: &Src, o: &Opts, report: &mut Vec<String>) -> PageOut {
    let (iw, ih) = (src.w as f64, src.h as f64);
    let page_dpi = o.photo_dpi;
    let mut pw = py_round(210.0 / MM * page_dpi as f64) as usize;
    let mut ph = py_round(297.0 / MM * page_dpi as f64) as usize;
    let turned = iw > ih * 1.05 && !o.landscape;
    if o.landscape || turned {
        std::mem::swap(&mut pw, &mut ph);
    }
    let scale = (pw as f64 / iw).min(ph as f64 / ih);
    let off = (py_round((pw as f64 - iw * scale) / 2.0), py_round((ph as f64 - ih * scale) / 2.0));
    let p: f64 = format!("{:.4}", scale * 100.0).parse().unwrap();
    let mut scaled = img::resize_any(src, ops::pct(src.w, p), ops::pct(src.h, p));
    scaled.q8();
    let mut page = Img::solid(pw, ph, [1.0; 3]);
    page.over(&scaled, None, off.0 as isize, off.1 as isize);
    page.q8();
    if o.gray {
        report.push("--gray ignored: the photo path does not touch colour".into());
    }
    report.push(format!(
        "fit: frame (photo, {}); scale {:.4}, offset {:+}{:+}",
        if turned { "landscape page" } else { "kept as shot" },
        scale,
        off.0,
        off.1
    ));
    report.push(format!(
        "page: {}x{}px @ {}dpi (no flat-field, no tone, no paper-whitening, no sharpen — passthrough)",
        pw, ph, page_dpi
    ));
    PageOut { img: page, photo: true, dpi: page_dpi }
}

const LOWRES_DPI: f64 = 180.0;
const SPECK_MM: f64 = 2.5;
const SPECK_REACH_MM: f64 = 3.0;
const SPECK_DARK: f64 = 60.0;
const SPECK_DPI: f64 = 150.0;
const SPECK_CORNER: f64 = 25.0;
const SPECK_LINE: f64 = 4.0;
const SPECK_INK: f64 = 96.0;

/// Whiten small marks on open paper; return the share of the page cleaned.
fn clean_specks(page: &mut Img, dpi: usize) -> f64 {
    let (w, h) = (page.w, page.h);
    let f = 1f64.min(SPECK_DPI / dpi as f64);
    let aw = 1.max(py_round(w as f64 * f)) as usize;
    let ah = 1.max(py_round(h as f64 * f)) as usize;
    let px = dpi as f64 * f / MM;
    let area = (SPECK_MM * px).powi(2);
    let long_min = SPECK_MM * px;
    let (zw, zh) = (1.max(aw / 2), 1.max(ah / 2));
    let reach = 1.max(py_round(SPECK_REACH_MM * px / 2.0)) as usize;
    // the marks as bytes: a page-sized float mask costs four times as much
    let g = page.gray();
    let (dark_thr, ink_thr) = (ops::pct_thr(SPECK_DARK), ops::pct_thr(SPECK_INK));
    let dark = ops::threshold(
        &ops::resize_rows(w, h, &|y, out| {
            for (o, &v) in out.iter_mut().zip(g.row(y)) {
                *o = (v <= dark_thr) as u8 as f32;
            }
        }, zw, zh, Filter::Box),
        10.0,
    );
    let ink: Vec<u8> = g.d.iter().map(|&v| (v <= ink_thr) as u8).collect();
    drop(g);
    let small_ink = ops::threshold(
        &ops::resize_rows(w, h, &|y, out| {
            for (o, &v) in out.iter_mut().zip(&ink[y * w..(y + 1) * w]) {
                *o = v as f32;
            }
        }, aw, ah, Filter::Box),
        20.0,
    );
    let blobs = ops::components8(&small_ink.bytes(), aw, ah);
    let (mut rects, mut small) = (vec![], 0);
    let (cx_max, cy_max) = (aw as f64 * SPECK_CORNER / 100.0, ah as f64 * SPECK_CORNER / 100.0);
    for b in blobs {
        let (bw, bh, bx, by) = (b.w as f64, b.h as f64, b.x as f64, b.y as f64);
        let (lo, hi) = (bw.min(bh), bw.max(bh));
        if !(b.area as f64 >= area || (hi >= long_min && hi >= SPECK_LINE * lo)) {
            small += 1;
            continue;
        }
        let in_x = bx + bw <= cx_max || bx >= aw as f64 - cx_max;
        let in_y = by + bh <= cy_max || by >= ah as f64 - cy_max;
        let edge = bx <= 1.0 || by <= 1.0 || bx + bw >= aw as f64 - 1.0 || by + bh >= ah as f64 - 1.0;
        if in_x && in_y && edge {
            small += 1;
            continue;
        }
        rects.push((b.x / 2, b.y / 2, (b.x + b.w - 1) / 2, (b.y + b.h - 1) / 2));
    }
    if small == 0 {
        return 0.0;
    }
    let mut zone = Plane::new(zw, zh);
    for (x0, y0, x1, y1) in rects {
        let x1 = x1.min(zw - 1);
        for y in y0..=y1.min(zh - 1) {
            for x in x0..=x1 {
                zone.d[y * zw + x] = 1.0;
            }
        }
    }
    let prot = zone.zip(&dark, f32::max);
    let prot = ops::morph_p(&prot, &ops::disk(reach), true, 1);
    // an ink pixel no substance protects: -sample of the zone, negated, times the ink
    let sp = ops::sample_u8(&prot.bytes(), zw, zh, w, h);
    let mut specks = ink;
    for (s, &p) in specks.iter_mut().zip(&sp.d) {
        *s &= (p < 0.5) as u8;
    }
    drop(sp);
    let specks = ops::morph_k(&specks, w, h, &ops::square(1), true);
    let share = specks.iter().map(|&v| v as f64).sum::<f64>() / (w * h) as f64 * 100.0;
    finish::whiten(page, &specks);
    page.q8();
    share
}

const PATCH_MONO: u8 = 20;

/// A face photo kept for the end: its crop from the page before the
/// flat-field, its box, and that page's paper level.
pub struct Keep {
    crop: Img,
    bx: (i64, i64, i64, i64),
    paper: f64,
}

fn keep_of(page: &Img, bx: (i64, i64, i64, i64)) -> Option<Keep> {
    let (x, y, w, h) = bx;
    let gray = d::raw_gray(page, 200, 1.max(py_round(200.0 * page.h as f64 / page.w as f64)) as usize);
    let mut sg = gray;
    sg.sort_unstable();
    let paper = sg[(0.90 * (sg.len() - 1) as f64) as usize] as f64;
    // -crop is clipped to the image
    let x0 = x.clamp(0, page.w as i64 - 1) as usize;
    let y0 = y.clamp(0, page.h as i64 - 1) as usize;
    let x1 = ((x + w).max(0) as usize).min(page.w);
    let y1 = ((y + h).max(0) as usize).min(page.h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Keep { crop: page.crop(x0, y0, x1 - x0, y1 - y0), bx, paper })
}

/// The face photo, toned on its own, and where it goes on the page.
fn photo_patch(keep: &Keep, scale: f64, off: (i64, i64), o: &Opts) -> Option<(Img, Plane, isize, isize)> {
    let (bx, by, bw, bh) = keep.bx;
    let paper = keep.paper;
    let cw = 120;
    let chh = 1.max(py_round(cw as f64 * bh as f64 / bw as f64)) as usize;
    let crop = keep.crop.clone();
    let g = ops::resize_auto(&crop.gray(), cw, chh).bytes();
    let mut sgs = g.clone();
    sgs.sort_unstable();
    let black = sgs[(0.005 * (sgs.len() - 1) as f64) as usize] as f64;
    let rgb = crop.resize_auto(cw, chh).rgb().to_rgb8();
    let mut chroma: Vec<u8> = rgb.chunks(3).map(|p| p.iter().max().unwrap() - p.iter().min().unwrap()).collect();
    chroma.sort_unstable();
    let ch95 = chroma[(0.95 * (chroma.len() - 1) as f64) as usize];
    let mono = o.gray || ch95 < PATCH_MONO;
    let light = sgs[(0.85 * (sgs.len() - 1) as f64) as usize] as f64;
    let white = (0.98 * paper).min((0.85 * paper).max(light));
    let lo = 100.0 * black / 255.0;
    let hi = (lo + 10.0).max(100.0 * white / 255.0);
    let pw_ = 1.max(py_round(bw as f64 * scale)) as usize;
    let ph_ = 1.max(py_round(bh as f64 * scale)) as usize;
    let feather = 2.max(py_round(pw_.min(ph_) as f64 * 0.015)) as usize;
    let base = if mono { Img::from_planes(vec![crop.gray()]) } else { crop };
    let r2 = |v: f64| format!("{:.2}", v).parse::<f64>().unwrap();
    let patch = base.level(r2(lo), r2(hi)).resize_auto(pw_, ph_).each(|p| ops::unsharp(p, 1.0, o.sharpen as f32, 0.02)).rgb();
    let f2 = 2 * feather as isize;
    let rect = img::rect_mask(pw_, ph_, f2, f2, pw_ as isize - 1 - f2, ph_ as isize - 1 - f2);
    let alpha = ops::blur(&rect, feather as f64);
    let x = off.0 + py_round(bx as f64 * scale);
    let y = off.1 + py_round(by as f64 * scale);
    Some((patch, alpha, x as isize, y as isize))
}

// ---------------------------------------------------------- the page pipeline

/// One page on its way through the pipeline.
pub struct Job<'a> {
    pub cur: Img,
    /// the canvas offset a deskew leaves behind (-page); a crop resets it,
    /// and -trim reports the ink box in its coordinates
    pub page: (isize, isize),
    pub o: &'a Opts,
    pub report: Vec<String>,
}

impl Job<'_> {
    fn say(&mut self, s: String) {
        self.report.push(s);
    }
}

pub enum Found {
    Photo,
    Cards(Vec<Quad>, String),
    Spread(d::Spread),
    Sheet(Quad, String),
    Frame,
}

/// What is in the frame, before anything touches the pixels.
pub fn locate(src: &Src, o: &Opts, report: &mut Vec<String>) -> Result<Found, Fail> {
    if o.photo == "on" {
        report.push("--photo on — kept as a photo, not scanned".into());
        return Ok(Found::Photo);
    }
    if o.rectify == "off" {
        return Ok(Found::Frame);
    }
    let vc = d::vc(src, 400);
    let pm = d::paper_mask(&vc, d::Mode::Paper);
    let mut spread = None;
    if o.spread != "off" {
        let mut whys = vec![];
        for mode in [d::Mode::Paper, d::Mode::Tinted, d::Mode::Otsu] {
            let cand;
            let pmr = if mode == d::Mode::Paper {
                &pm
            } else {
                cand = d::paper_mask(&vc, mode);
                &cand
            };
            match d::detect_spread(pmr) {
                Ok(mut s) => {
                    if mode != d::Mode::Paper {
                        s.why += &format!(", {} paper mask", mode.name());
                    }
                    spread = Some(s);
                    break;
                }
                Err(why) => whys.push(format!("{}: {}", mode.name(), why)),
            }
        }
        if spread.is_none() && o.spread == "on" {
            return Err(Fail(format!("a4norm: --spread on, but no two facing pages found: {}", whys.join("; "))));
        }
    }
    if o.cards != "off" {
        let mut cq = d::detect_cards(&vc).ok();
        if let (Some((cquads, _)), Some(s)) = (&cq, &spread) {
            if cquads.len() == 1 {
                let sarea: f64 = s.quads.iter().map(|q| d::shoelace(q)).sum();
                let ca = d::shoelace(&cquads[0]);
                if d::CARD_OF_SPREAD * sarea <= ca && ca <= 1.25 * sarea && s.why.contains("folded in the middle") {
                    spread = None;
                }
            }
        }
        if let (Some((cquads, _)), Some(_)) = (&cq, &spread) {
            if cquads.len() == 1 {
                cq = None;
            }
        }
        if let Some((cquads, cwhy)) = cq {
            if cquads.len() == 2 || spread.is_none() {
                let alone = d::cards_alone(src, &cquads, report);
                if alone.len() == cquads.len() {
                    if let Some(s) = &spread {
                        report.push(format!("two cards, not a spread ({})", s.why));
                    }
                    return Ok(Found::Cards(cquads, cwhy));
                }
            }
        }
    }
    if let Some(s) = spread {
        return Ok(Found::Spread(s));
    }
    let (mut quad, mut why, paper_share) = d::quad_from_mask(&pm);
    if o.edges != "off" {
        match d::edge_document(src, quad, o, report) {
            Some(d::EdgeDoc::Cards(q, w)) => return Ok(Found::Cards(q, w)),
            Some(d::EdgeDoc::Spread(s)) => return Ok(Found::Spread(s)),
            Some(d::EdgeDoc::Sheet(q, w)) => {
                quad = Some(q);
                why = w;
            }
            None => {}
        }
    }
    let mut paper_share = paper_share;
    if quad.is_none() {
        // brightness could not find the sheet: the segmentation helper, if
        // this build has one, judged by the same rules
        let (sq, swhy, sshare) = match o.seg {
            Some(f) => f(src),
            None => (None, "no segmentation helper installed".to_string(), 0.0),
        };
        if let Some(q) = sq {
            quad = Some(q);
            why = format!("{}, found by segmentation after brightness failed ({})", swhy, why);
            paper_share = paper_share.max(sshare);
        } else if !swhy.contains("no segmentation helper") {
            report.push(format!("segmentation did not find a sheet either: {}", swhy));
        }
    }
    if let Some(q) = quad {
        return Ok(Found::Sheet(q, why));
    }
    if o.rectify == "on" {
        return Err(Fail(format!("a4norm: --rectify on, but no sheet quad found: {}", why)));
    }
    report.push(format!("no rectification: {}", why));
    if o.photo == "auto" && paper_share * 100.0 < o.photo_paper {
        report.push(format!(
            "no document in the frame (paper-like area {}, under --photo-paper {:.0}%) — kept as a photo, not scanned",
            pc0(paper_share),
            o.photo_paper
        ));
        return Ok(Found::Photo);
    }
    report.push(format!(
        "paper-like area {} >= --photo-paper {:.0}% — treated as a document",
        pc0(paper_share),
        o.photo_paper
    ));
    Ok(Found::Frame)
}

fn rectify_spread(job: &mut Job, src: &Src, s: &d::Spread) -> (usize, usize) {
    let o = job.o;
    let pw_ = s.quads.iter().map(|q| (dist(q[0], q[1]) + dist(q[3], q[2])) / 2.0).sum::<f64>() / 2.0;
    let ph_ = s.quads.iter().map(|q| (dist(q[0], q[3]) + dist(q[1], q[2])) / 2.0).sum::<f64>() / 2.0;
    let (sw, sh) = if s.horiz { (2.0 * pw_, ph_) } else { (pw_, 2.0 * ph_) };
    let mut k = 1.0;
    if o.fit == "auto" {
        let cap = a4_px(o.dpi as f64);
        let (cmax, cmin) = (cap.0.max(cap.1) as f64, cap.0.min(cap.1) as f64);
        k = 1f64.min(cmax / sw.max(sh)).min(cmin / sw.min(sh));
    }
    let w1 = 50.max(py_round(pw_ * k)) as usize;
    let h1 = 50.max(py_round(ph_ * k)) as usize;
    let halves: Vec<Img> = ops::par_map(2, |i| rectify(src, &s.quads[i], o.rect_inset, None, false, Some((w1, h1))).0);
    let mut cur = Img::append(&halves[0], &halves[1], s.horiz);
    cur.q8();
    job.cur = cur;
    let (w, h) = (job.cur.w, job.cur.h);
    job.say(format!(
        "spread: two facing pages ({}), each rectified to {}x{} and joined at the fold -> {}x{}",
        s.why, w1, h1, w, h
    ));
    let (got, share) = erase_fingers(&mut job.cur, Some(s.horiz));
    if got {
        job.say(format!("erased a finger at the outer edge ({:.1}% of the spread)", share));
    } else if share > FINGER_MAX {
        job.say(format!(
            "finger erase skipped: what looked like skin was {:.0}% of the spread — the pages' own colour",
            share
        ));
    }
    (w, h)
}

fn rectify_sheet(job: &mut Job, src: &Src, quad: &Quad, why: &str) -> (usize, usize) {
    let o = job.o;
    let mut cap = None;
    if o.fit == "auto" {
        let c = a4_px(o.dpi as f64);
        cap = Some(if o.landscape { (c.1, c.0) } else { c });
    }
    let (img, w, h) = rectify(src, quad, o.rect_inset, cap, o.rotate == "auto" && !o.landscape, None);
    job.cur = img;
    job.say(format!("rectified the sheet quad ({}) -> {}x{}", why, w, h));
    (w, h)
}

fn erase_outside(job: &mut Job, w: usize, h: usize) {
    let o = job.o;
    if o.no_edge_clean {
        return;
    }
    let b = clean_border(&mut job.cur, o.edge_band, o.edge_keep, o.band_structure, o.band_dark);
    if let Some(v) = b.verdict {
        let kept: Vec<String> = SIDES
            .iter()
            .enumerate()
            .filter(|(k, _)| v[*k].doc && v[*k].cnt > 0)
            .map(|(k, s)| format!("{} (structure {:.1}%, brightness {:.0}% of paper)", s, v[k].st, v[k].dk))
            .collect();
        if !kept.is_empty() {
            job.say(format!("kept as document, not erased or cut: {}", kept.join("; ")));
        }
    }
    if !b.done {
        if b.share != 0.0 {
            job.say(format!("border cleanup skipped: it would have repainted {:.0}% of the page", b.share));
        }
        return;
    }
    job.say(format!("erased what leaned in from outside the sheet ({:.1}% of the page)", b.share));
    let pad = py_round(w.max(h) as f64 * 0.004) as usize;
    let add = |c: usize| if c > 0 { c + pad } else { 0 };
    let (l, r, t, bb) = (add(b.cut[0]), add(b.cut[1]), add(b.cut[2]), add(b.cut[3]));
    if (l + r + t + bb) > 0 && w as isize - (l + r) as isize > 100 && h as isize - (t + bb) as isize > 100 {
        let mut c = job.cur.crop(l, t, w - l - r, h - t - bb);
        c.q8();
        job.cur = c;
        job.say(format!("cut a solid band of it away: L/R/T/B = {}/{}/{}/{} px", l, r, t, bb));
        let b2 = clean_border(&mut job.cur, o.edge_band, o.edge_keep, o.band_structure, o.band_dark);
        if b2.done {
            job.say(format!("second pass on the new border ({:.1}%)", b2.share));
        }
    }
}

fn orient(job: &mut Job, spread: Option<&d::Spread>) {
    let o = job.o;
    if o.rotate != "auto" && o.rotate != "0" {
        let deg: i32 = o.rotate.parse().unwrap_or(0);
        job.cur = job.cur.rotate(deg);
        job.say(format!("rotated {}°", o.rotate));
    } else if let (Some(s), "auto") = (spread, o.rotate.as_str()) {
        let turn = d::spread_turn(&job.cur, s.horiz, &mut job.report);
        if turn != 0 {
            job.cur = job.cur.rotate(turn);
            job.say(format!("rotated {}°", turn));
        }
    }
}

pub struct Trim {
    flags: [bool; 4],
    span: (usize, usize),
    shave: (usize, usize),
}

fn trim_border(job: &mut Job) -> Trim {
    let o = job.o;
    let mut t = Trim { flags: [false; 4], span: (0, 0), shave: (0, 0) };
    if o.no_trim {
        return t;
    }
    let (dl, dr, dt, db, flags) = d::detect_border(&job.cur, o.trim_band / 100.0, o.trim_step);
    let (iw, ih) = (job.cur.w, job.cur.h);
    t.flags = flags;
    t.span = (iw.saturating_sub(dl + dr), ih.saturating_sub(dt + db));
    let sh = (o.trim_pad as i64).max(py_round(o.trim_shave / 100.0 * iw.min(ih) as f64)) as usize;
    let cut = [dl, dr, dt, db].iter().zip(flags).map(|(&v, f)| if f { v + sh } else { 0 }).collect::<Vec<_>>();
    t.shave = (if cut[0] > 0 { sh } else { 0 }, if cut[2] > 0 { sh } else { 0 });
    if cut.iter().any(|&c| c > 0) {
        let mut c = job.cur.crop(cut[0], cut[2], iw - cut[0] - cut[1], ih - cut[2] - cut[3]);
        c.q8();
        job.cur = c;
    }
    let names = ["left", "right", "top", "bottom"];
    // the flags in the script's dict order: top, bottom, left, right
    let order = [2usize, 3, 0, 1];
    let found: Vec<&str> = order.iter().filter(|&&k| flags[k]).map(|&k| names[k]).collect();
    job.say(format!(
        "border cut L/R/T/B = {}/{}/{}/{} px (edge + {} px shave; edges: {})",
        cut[0],
        cut[1],
        cut[2],
        cut[3],
        sh,
        if found.is_empty() { "none".to_string() } else { found.join(",") }
    ));
    t
}

fn spread_photo_box(job: &Job) -> Option<(i64, i64, i64, i64)> {
    let (iw, ih) = (job.cur.w, job.cur.h);
    let x1 = py_round(iw as f64 * 0.45) as usize;
    let y0 = py_round(ih as f64 * 0.5) as usize;
    let mut part = job.cur.crop(0, y0, x1, ih - y0);
    part.q8();
    let asp = Some((0.45, 1.8));
    let got = d::find_photo_block(&part, Some(8.0), Some(65.0), None, asp)
        .or_else(|| d::find_photo_block(&part, Some(8.0), Some(65.0), Some(0.25), asp))
        .or_else(|| d::find_photo_block(&part, Some(3.0), Some(65.0), None, asp))?;
    let (bx, by, mut bw, mut bh) = (got.0, got.1 + y0 as i64, got.2, got.3);
    let pw_ = py_round(iw as f64 * d::SPREAD_PHOTO_MM.0 / 88.0);
    let ph_ = py_round(ih as f64 / 2.0 * d::SPREAD_PHOTO_MM.1 / 125.0);
    if bw as f64 > 1.25 * pw_ as f64 {
        bw = py_round(1.1 * pw_ as f64);
    }
    if bh < ph_ {
        bh = ph_.min(ih as i64 - by);
    }
    Some((bx, by, bw, bh))
}

fn keep_face_photo(job: &mut Job, spread: bool) -> Option<Keep> {
    let bx = if spread { spread_photo_box(job) } else { d::find_photo_block(&job.cur, None, None, None, None) }?;
    let (pw0, ph0) = (job.cur.w, job.cur.h);
    job.say(format!(
        "face photo at {}x{}+{}+{} of {}x{} — toned on its own, not flattened into the paper",
        bx.2, bx.3, bx.0, bx.1, pw0, ph0
    ));
    keep_of(&job.cur, bx)
}

fn deskew(job: &mut Job, t: &mut Trim, keep: Option<Keep>) -> Option<Keep> {
    let o = job.o;
    let ang = d::deskew_angle(&job.cur);
    if !(o.deskew_min <= ang.abs() && ang.abs() <= 5.0) {
        job.say(format!("skew {:+.2}° — left as is", ang));
        return keep;
    }
    let before = (job.cur.w, job.cur.h);
    // the stage measures again, on the colour page at 40%
    let deg = {
        let ch: Vec<&Plane> = job.cur.c.iter().collect();
        ops::deskew_angle(&ch, ops::pct_thr(40.0))
    };
    let (mut r, px, py) = img::rotate_fit(&job.cur, deg);
    r.q8();
    job.cur = r;
    job.page = (px, py);
    let after = (job.cur.w, job.cur.h);
    if t.span != (0, 0) && after != before {
        t.span.0 = py_round(t.span.0 as f64 * after.0 as f64 / before.0 as f64) as usize;
        t.span.1 = py_round(t.span.1 as f64 * after.1 as f64 / before.1 as f64) as usize;
    }
    let lean = (after.0.max(after.1) as f64 * ang.to_radians().sin().abs()).ceil() as usize;
    if lean > 0 && t.flags.iter().any(|&f| f) {
        let (iw2, ih2) = after;
        let cl = if t.flags[0] { lean } else { 0 };
        let cr = if t.flags[1] { lean } else { 0 };
        let ct = if t.flags[2] { lean } else { 0 };
        let cb = if t.flags[3] { lean } else { 0 };
        if iw2 as isize - (cl + cr) as isize > 100 && ih2 as isize - (ct + cb) as isize > 100 {
            // -crop counts from the canvas the turn left behind (its offset
            // is negative), and is clipped to the pixels there are
            let x0 = (cl as isize - px).clamp(0, iw2 as isize - 1) as usize;
            let y0 = (ct as isize - py).clamp(0, ih2 as isize - 1) as usize;
            let cw = (iw2 - cl - cr).min(iw2 - x0);
            let chh = (ih2 - ct - cb).min(ih2 - y0);
            let mut c = job.cur.crop(x0, y0, cw, chh);
            c.q8();
            job.cur = c;
            job.page = (0, 0);
            t.shave.0 += cl;
            t.shave.1 += ct;
        }
    }
    job.say(format!("deskewed {:+.2}°, edge lean cut {}px", ang, lean));
    if keep.is_some() {
        job.say("face photo left to the page treatment: the page was deskewed after the photo was cut out".into());
        return None;
    }
    keep
}

struct Fit {
    pw: usize,
    ph: usize,
    scale: f64,
    off: (i64, i64),
    mode: String,
    dpi: usize,
    lowres: Option<f64>,
}

fn choose_fit(job: &Job, rectified: bool, t: &Trim) -> Fit {
    let o = job.o;
    let (iw, ih) = (job.cur.w as f64, job.cur.h as f64);
    let mut page_dpi = o.dpi;
    let (mut pw, mut ph) = a4_px(page_dpi as f64);
    let turned = iw > ih * (1.0 + o.rotate_tol / 100.0) && !o.landscape;
    if o.landscape || turned {
        std::mem::swap(&mut pw, &mut ph);
    }
    let (mut scale, mut off_x, mut off_y, mut mode): (Option<f64>, Option<i64>, Option<i64>, Option<String>) = (None, None, None, None);
    let [fl, fr, ft, fb] = t.flags;
    if rectified && o.fit == "auto" {
        scale = Some((pw as f64 / iw).min(ph as f64 / ih));
        mode = Some("frame (the rectified quad is the sheet)".into());
    }
    if scale.is_none() && (o.fit == "auto" || o.fit == "edges") {
        if ft && fb {
            let s = ph as f64 / t.span.1 as f64;
            scale = Some(s);
            off_y = Some(py_round(t.shave.1 as f64 * s));
            mode = Some(format!("edges (sheet height = {}px)", t.span.1));
        } else if fl && fr {
            let s = pw as f64 / t.span.0 as f64;
            scale = Some(s);
            off_x = Some(py_round(t.shave.0 as f64 * s));
            mode = Some(format!("edges (sheet width = {}px)", t.span.0));
        } else if o.fit == "edges" {
            mode = Some("frame (no pair of opposite edges found)".into());
        }
    }
    if scale.is_none() && (o.fit == "auto" || o.fit == "content") {
        if o.fit == "auto" && (fl || fr || ft || fb) {
            mode = Some("frame (a sheet edge was found; no text width to guess)".into());
        } else {
            if let Some((bx, by, bw, _)) = d::ink_bbox(&job.cur, 78.0) {
                let (bx, by) = (bx as isize + job.page.0, by as isize + job.page.1);
                if bw as f64 >= o.content_min / 100.0 * iw {
                    let text_w = (if o.landscape { 297.0 } else { 210.0 }) - o.ml - o.mr;
                    let s = (text_w / MM * o.dpi as f64) / bw as f64;
                    if (0.4..=3.0).contains(&s) {
                        scale = Some(s);
                        off_x = Some(py_round(o.ml / MM * o.dpi as f64 - bx as f64 * s));
                        off_y = Some(py_round(o.mt / MM * o.dpi as f64 - by as f64 * s));
                        mode = Some(format!(
                            "content ({}px ink block -> {:.0}mm text width at {:.0}/{:.0}/{:.0}mm)",
                            bw, text_w, o.ml, o.mr, o.mt
                        ));
                    }
                }
            }
            if scale.is_none() {
                mode = Some("frame (no ink block wide enough to anchor on)".into());
            }
        }
    }
    let mut scale = match scale {
        Some(s) => s,
        None => {
            if mode.is_none() {
                mode = Some("frame".into());
            }
            (pw as f64 / iw).min(ph as f64 / ih)
        }
    };
    let mut off = (
        off_x.unwrap_or_else(|| py_round((pw as f64 - iw * scale) / 2.0)),
        off_y.unwrap_or_else(|| py_round((ph as f64 - ih * scale) / 2.0)),
    );
    let mut lowres = None;
    if !o.dpi_given && scale > o.dpi as f64 / LOWRES_DPI && o.dpi > 200 {
        let wide = pw > ph;
        (pw, ph) = a4_px(200.0);
        if wide {
            std::mem::swap(&mut pw, &mut ph);
        }
        let full = a4_px(o.dpi as f64);
        let f = pw as f64 / (if wide { full.1 } else { full.0 }) as f64;
        scale *= f;
        off = (py_round(off.0 as f64 * f), py_round(off.1 as f64 * f));
        page_dpi = 200;
        lowres = Some(f);
    }
    Fit { pw, ph, scale, off, mode: mode.unwrap(), dpi: page_dpi, lowres }
}

fn lay_out(job: &mut Job, rectified: bool, t: &Trim, keep: Option<Keep>, copy: bool) -> (Img, usize) {
    let o = job.o;
    let f = choose_fit(job, rectified, t);
    let over = keep.as_ref().and_then(|k| photo_patch(k, f.scale, f.off, o));
    let p: f64 = format!("{:.4}", f.scale * 100.0).parse().unwrap();
    // one channel at a time: scaled, sharpened, laid on the page, dropped
    let cur = std::mem::replace(&mut job.cur, Img::solid(1, 1, [1.0; 3]));
    let (nw, nh) = (ops::pct(cur.w, p), ops::pct(cur.h, p));
    let mut page = Img::solid(f.pw, f.ph, [1.0; 3]);
    let grey = cur.c.len() == 1;
    for (k, plane) in cur.c.into_iter().enumerate() {
        let mut r = ops::resize_auto(&plane, nw, nh);
        drop(plane);
        ops::unsharp_in(&mut r, 1.0, o.sharpen as f32, 0.02);
        for kk in if grey { 0..3 } else { k..k + 1 } {
            page.c[kk].paste(&r, f.off.0 as isize, f.off.1 as isize);
        }
    }
    if !copy {
        finish::paper_screen(&mut page, o.paper_thr);
    }
    page.q8();
    if !o.no_despeckle && !copy {
        let share = clean_specks(&mut page, f.dpi);
        if share != 0.0 {
            job.say(format!("cleaned {:.3}% of the page of specks on open paper", share));
        }
    }
    if let Some((patch, alpha, x, y)) = over {
        page.over(&patch, Some(&alpha), x, y);
        page.q8();
    }
    job.say(format!("fit: {}; scale {:.4}, offset {:+}{:+}", f.mode, f.scale, f.off.0, f.off.1));
    job.say(format!("page: {}x{}px @ {}dpi", f.pw, f.ph, f.dpi));
    if let Some(lr) = f.lowres {
        job.say(format!(
            "the photo holds about {:.0} dpi of detail at this size — page written at {} dpi, not {} (--dpi forces it)",
            o.dpi as f64 / (f.scale / lr),
            f.dpi,
            o.dpi
        ));
    }
    (page, f.dpi)
}

pub enum Processed {
    Page(PageOut),
    Cards(Vec<Card>),
}

/// Progress: the stage just finished.
pub type Progress<'a> = &'a dyn Fn(&str);

/// One raster -> one A4 page, or the cards found in it. The photo is
/// dropped as soon as the page no longer needs it.
pub fn process_page(src: Src, o: &Opts, report: &mut Vec<String>, step: Progress) -> Result<Processed, Fail> {
    let found = locate(&src, o, report)?;
    step("locate");
    let (spread, sheet) = match found {
        Found::Photo => return Ok(Processed::Page(fit_photo_page(&src, o, report))),
        Found::Cards(q, why) => {
            let c = process_cards(&src, &q, &why, o, report);
            step("cards");
            return Ok(Processed::Cards(c));
        }
        Found::Spread(s) => (Some(s), None),
        Found::Sheet(q, w) => (None, Some((q, w))),
        Found::Frame => (None, None),
    };
    let rectified = spread.is_some() || sheet.is_some();
    // the frame path works on the whole photo; a rectify makes its own page
    let cur = if rectified { Img::solid(1, 1, [1.0; 3]) } else { src.to_img() };
    let mut job = Job { cur, page: (0, 0), o, report: std::mem::take(report) };
    let mut wh = (0, 0);
    if let Some(s) = &spread {
        wh = rectify_spread(&mut job, &src, s);
    } else if let Some((q, w)) = &sheet {
        wh = rectify_sheet(&mut job, &src, q, w);
    }
    drop(src);
    if rectified {
        step("rectify");
        erase_outside(&mut job, wh.0, wh.1);
        step("border");
    }
    orient(&mut job, spread.as_ref());
    let mut t = if rectified { Trim { flags: [false; 4], span: (0, 0), shave: (0, 0) } } else { trim_border(&mut job) };
    let copy = spread.is_some() && !o.spread_scan;
    if copy {
        let w = job.cur.w;
        let mut c = copy_tone(&job.cur, w);
        c.q8();
        job.cur = c;
        job.say("colour copy: light evened, tint and security print kept, nothing whitened".into());
    }
    let mut keep = if o.no_keep_photo || copy { None } else { keep_face_photo(&mut job, spread.is_some()) };
    if !copy {
        finish::flat_field(&mut job.cur, o);
        step("flat");
    }
    if !o.no_deskew && !rectified {
        keep = deskew(&mut job, &mut t, keep);
    }
    if o.gray {
        let g = job.cur.gray();
        job.cur = Img::from_planes(vec![g.clone(), g.clone(), g]);
        job.cur.q8();
        job.say("grayscale output".into());
    } else if !o.no_neutralize && !copy {
        let share = finish::neutralize_ink(&mut job.cur, o);
        job.say(format!("ink neutralized, coloured ink kept on {:.3}% of the page", share));
        step("ink");
    }
    if !copy {
        finish::tone(&mut job.cur, o);
        step("tone");
    }
    let (page, dpi) = lay_out(&mut job, rectified, &t, keep, copy);
    step("page");
    *report = job.report;
    Ok(Processed::Page(PageOut { img: page, photo: false, dpi }))
}
