//! What is in the frame: paper masks, blobs, the sheet's quadrilateral, an
//! open booklet's two pages, ID-1 cards, and documents found by their edges.
//! A line-for-line port of the script's "analysis", "spreads", "cards" and
//! "edge finding" sections; the constants carry the same names.

use crate::img::{self, Img, Pix};
use crate::ops;
use crate::Opts;

pub type Pt = (f64, f64);
pub type Quad = [Pt; 4];

fn dist(a: Pt, b: Pt) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// `f"{x:.0%}"`
pub fn pc0(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

// ------------------------------------------------------------ small copies

/// `magick P -colorspace gray -resize WxH! -depth 8 gray:-`
pub fn raw_gray(img: &dyn Pix, w: usize, h: usize) -> Vec<u8> {
    img::gray_resized(img, w, h).bytes()
}

/// `magick P -resize WxH! -depth 8 rgb:-`, as (V, C, rgb): per pixel the
/// value (max channel), the chroma (max - min) and the bytes themselves.
pub fn raw_rgb(img: &dyn Pix, w: usize, h: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let small = img::resize_any(img, w, h).rgb().to_rgb8();
    let n = w * h;
    let mut v = vec![0u8; n];
    let mut c = vec![0u8; n];
    for i in 0..n {
        let (r, g, b) = (small[3 * i], small[3 * i + 1], small[3 * i + 2]);
        let mx = r.max(g).max(b);
        v[i] = mx;
        c[i] = mx - r.min(g).min(b);
    }
    (v, c, small)
}

/// Brightness and chroma of a copy `side` px wide.
pub struct Vc {
    pub v: Vec<u8>,
    pub c: Vec<u8>,
    pub w: usize,
    pub h: usize,
    pub w0: usize,
    pub h0: usize,
}

pub fn vc(img: &dyn Pix, side: usize) -> Vc {
    let (w0, h0) = img.dims();
    let w = side;
    let h = (ops::py_round(side as f64 * h0 as f64 / w0 as f64)).max(1) as usize;
    let (v, c, _) = raw_rgb(img, w, h);
    Vc { v, c, w, h, w0, h0 }
}

fn kth(v: &[u8], frac: f64) -> u8 {
    let mut s = v.to_vec();
    s.sort_unstable();
    s[(frac * (s.len() - 1) as f64) as usize]
}

pub fn otsu(v: &[u8]) -> usize {
    let mut hist = [0usize; 256];
    for &x in v {
        hist[x as usize] += 1;
    }
    let n = v.len();
    let tot: usize = hist.iter().enumerate().map(|(i, c)| i * c).sum();
    let (mut sb, mut wb) = (0usize, 0usize);
    let (mut best, mut thr) = (-1.0f64, 128usize);
    for t in 0..256 {
        wb += hist[t];
        if wb == 0 {
            continue;
        }
        let wf = n - wb;
        if wf == 0 {
            break;
        }
        sb += t * hist[t];
        let mb = sb as f64 / wb as f64;
        let mf = (tot - sb) as f64 / wf as f64;
        let var = wb as f64 * wf as f64 * (mb - mf).powi(2);
        if var > best {
            best = var;
            thr = t;
        }
    }
    thr
}

/// 4-neighbour erode then dilate; the outermost row and column stay 0.
pub fn open4(m: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut er = vec![0u8; w * h];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w - 1 {
            let i = y * w + x;
            er[i] = m[i] & m[i - 1] & m[i + 1] & m[i - w] & m[i + w];
        }
    }
    let mut di = vec![0u8; w * h];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w - 1 {
            let i = y * w + x;
            di[i] = er[i] | er[i - 1] | er[i + 1] | er[i - w] | er[i + w];
        }
    }
    di
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Paper,
    Tinted,
    Otsu,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Paper => "paper",
            Mode::Tinted => "tinted",
            Mode::Otsu => "otsu",
        }
    }
}

pub struct Pm {
    pub mask: Vec<u8>,
    pub w: usize,
    pub h: usize,
    pub w0: usize,
    pub h0: usize,
}

/// The paper-like pixels of the frame.
pub fn paper_mask(vc: &Vc, mode: Mode) -> Pm {
    let n = vc.w * vc.h;
    let paper = kth(&vc.v, 0.95) as usize;
    let (v_thr, c_max) = match mode {
        Mode::Otsu => (otsu(&vc.v) + 1, 100),
        Mode::Paper => (60.max((paper as f64 * 0.72) as usize), 45),
        Mode::Tinted => (60.max((paper as f64 * 0.72) as usize), 100),
    };
    let mut m = vec![0u8; n];
    for i in 0..n {
        if vc.v[i] as usize >= v_thr && vc.c[i] as usize <= c_max {
            m[i] = 1;
        }
    }
    Pm { mask: open4(&m, vc.w, vc.h), w: vc.w, h: vc.h, w0: vc.w0, h0: vc.h0 }
}

/// 4-connected blobs, largest first (ties in the order found), each in the
/// order the flood met its pixels: right, left, down, up.
pub fn components(m: &[u8], w: usize, h: usize) -> Vec<Vec<usize>> {
    let n = w * h;
    let mut seen = vec![false; n];
    let mut comps = vec![];
    for s0 in 0..n {
        if m[s0] == 0 || seen[s0] {
            continue;
        }
        seen[s0] = true;
        let mut comp = vec![s0];
        let mut head = 0;
        while head < comp.len() {
            let i = comp[head];
            head += 1;
            let x = i % w;
            let mut visit = |j: usize, comp: &mut Vec<usize>| {
                if m[j] != 0 && !seen[j] {
                    seen[j] = true;
                    comp.push(j);
                }
            };
            if x + 1 < w {
                visit(i + 1, &mut comp);
            }
            if x > 0 {
                visit(i - 1, &mut comp);
            }
            if i + w < n {
                visit(i + w, &mut comp);
            }
            if i >= w {
                visit(i - w, &mut comp);
            }
        }
        comps.push(comp);
    }
    comps.sort_by(|a, b| b.len().cmp(&a.len()));
    comps
}

pub fn shoelace(p: &[Pt]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (x1, y1) = p[i];
        let (x2, y2) = p[(i + 1) % p.len()];
        a += x1 * y2 - x2 * y1;
    }
    a.abs() / 2.0
}

/// Angle at b, in degrees.
pub fn angle(a: Pt, b: Pt, c: Pt) -> f64 {
    let v1 = (a.0 - b.0, a.1 - b.1);
    let v2 = (c.0 - b.0, c.1 - b.1);
    let n1 = v1.0.hypot(v1.1);
    let n2 = v2.0.hypot(v2.1);
    if n1 == 0.0 || n2 == 0.0 {
        return 0.0;
    }
    ((v1.0 * v2.0 + v1.1 * v2.1) / (n1 * n2)).clamp(-1.0, 1.0).acos().to_degrees()
}

/// The extreme pixel of `pts` by `key`, the first one on a tie (min) or the
/// first one on a tie (max): Python's min() and max() both keep the first.
fn arg_ext(pts: &[Pt], key: impl Fn(&Pt) -> f64, max: bool) -> Pt {
    let mut best = pts[0];
    let mut bk = key(&best);
    for p in &pts[1..] {
        let k = key(p);
        if (max && k > bk) || (!max && k < bk) {
            best = *p;
            bk = k;
        }
    }
    best
}

/// Largest blob -> sheet quad, or the reason there is none.
pub fn quad_from_mask(pm: &Pm) -> (Option<Quad>, String, f64) {
    let (w, h) = (pm.w, pm.h);
    let n = w * h;
    let comps = components(&pm.mask, w, h);
    let best = match comps.first() {
        Some(b) if !b.is_empty() => b,
        _ => return (None, "no bright low-chroma region found".into(), 0.0),
    };
    let frac = best.len() as f64 / n as f64;
    if frac > 0.90 {
        return (None, format!("the sheet already fills the frame ({})", pc0(frac)), frac);
    }
    if frac < 0.15 {
        return (None, format!("the brightest region is too small to be the sheet ({})", pc0(frac)), frac);
    }
    let pts: Vec<Pt> = best.iter().map(|&i| ((i % w) as f64, (i / w) as f64)).collect();
    let tl = arg_ext(&pts, |p| p.0 + p.1, false);
    let br = arg_ext(&pts, |p| p.0 + p.1, true);
    let tr = arg_ext(&pts, |p| p.0 - p.1, true);
    let bl = arg_ext(&pts, |p| p.0 - p.1, false);
    let quad = [tl, tr, br, bl];
    let mut uniq = quad.to_vec();
    uniq.sort_by(|a, b| a.partial_cmp(b).unwrap());
    uniq.dedup();
    if uniq.len() < 4 {
        return (None, "corners collapsed".into(), frac);
    }
    let qa = shoelace(&quad);
    if qa <= 0.0 || best.len() as f64 / qa < 0.80 {
        return (None, "the bright region is not shaped like a quadrilateral".into(), frac);
    }
    for i in 0..4 {
        let a = angle(quad[(i + 3) % 4], quad[i], quad[(i + 1) % 4]);
        if !(45.0..=135.0).contains(&a) {
            return (None, format!("corner angle {:.0}° is not a sheet corner", a), frac);
        }
    }
    let (top, bot) = (dist(tl, tr), dist(bl, br));
    let (lef, rig) = (dist(tl, bl), dist(tr, br));
    if top.max(bot) / 1e-6f64.max(top.min(bot)) > 1.8 || lef.max(rig) / 1e-6f64.max(lef.min(rig)) > 1.8 {
        return (None, "opposite sides differ too much to be one flat sheet".into(), frac);
    }
    let (sx, sy) = (pm.w0 as f64 / w as f64, pm.h0 as f64 / h as f64);
    let q = quad.map(|p| (p.0 * sx, p.1 * sy));
    (Some(q), format!("{} of the frame", pc0(frac)), frac)
}

// ------------------------------------------------------------------ spreads

pub const SPREAD_MIN: f64 = 10.0;
pub const SPREAD_BALANCE: f64 = 0.6;
pub const SPREAD_FILL: f64 = 0.45;
pub const EDGE_FRAME: f64 = 8.0;
const RIM: usize = 4;

/// The blob's pixels within RIM of its outside, in the blob's own order.
fn blob_rim(comp: &[usize], w: usize) -> Vec<usize> {
    if comp.is_empty() {
        return vec![];
    }
    let h = comp.iter().max().unwrap() / w + 1;
    let mut m = vec![0u8; w * h];
    for &i in comp {
        m[i] = 1;
    }
    for _ in 0..RIM {
        let mut e = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if m[i] == 0 {
                    continue;
                }
                let l = x > 0 && m[i - 1] != 0;
                let r = x + 1 < w && m[i + 1] != 0;
                let u = y > 0 && m[i - w] != 0;
                let d = y + 1 < h && m[i + w] != 0;
                e[i] = (l && r && u && d) as u8;
            }
        }
        m = e;
    }
    comp.iter().copied().filter(|&i| m[i] == 0).collect()
}

/// A blob and its rim, the rim worked out once.
pub struct Blob {
    pub comp: Vec<usize>,
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
}

impl Blob {
    pub fn new(comp: Vec<usize>, w: usize) -> Blob {
        let rim = blob_rim(&comp, w);
        Blob {
            xs: rim.iter().map(|&i| (i % w) as f64).collect(),
            ys: rim.iter().map(|&i| (i / w) as f64).collect(),
            comp,
        }
    }
    pub fn len(&self) -> usize {
        self.comp.len()
    }
}

/// The blob's outermost pixel per step along the side P->Q.
fn side_points(b: &Blob, w: usize, p: Pt, q: Pt, h: Option<usize>) -> Vec<Pt> {
    let l = dist(p, q);
    if l < 8.0 {
        return vec![];
    }
    let (ux, uy) = ((q.0 - p.0) / l, (q.1 - p.1) / l);
    let (nx, ny) = (uy, -ux);
    let (lo, hi) = (0.1 * l, 0.9 * l);
    let nk = l as usize + 2;
    let mut bs = vec![f64::NAN; nk];
    let mut bp = vec![(0.0, 0.0); nk];
    let mut order = vec![];
    for (&x, &y) in b.xs.iter().zip(&b.ys) {
        let (dx, dy) = (x - p.0, y - p.1);
        let t = dx * ux + dy * uy;
        if t < lo || t > hi {
            continue;
        }
        let sv = dx * nx + dy * ny;
        let k = t as usize;
        if bs[k].is_nan() {
            order.push(k);
            bs[k] = sv;
            bp[k] = (x, y);
        } else if sv > bs[k] {
            bs[k] = sv;
            bp[k] = (x, y);
        }
    }
    let wf = w as f64;
    order
        .into_iter()
        .map(|k| bp[k])
        .filter(|&(x, y)| match h {
            None => true,
            Some(h) => {
                EDGE_FRAME <= x && x < wf - EDGE_FRAME && EDGE_FRAME <= y && y < h as f64 - EDGE_FRAME
            }
        })
        .collect()
}

/// A line: centre, direction, residual, and (when fitted as an edge) support.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub cx: f64,
    pub cy: f64,
    pub dx: f64,
    pub dy: f64,
    pub res: f64,
    pub sup: f64,
}

impl Line {
    pub fn of(cx: f64, cy: f64, dx: f64, dy: f64) -> Line {
        Line { cx, cy, dx, dy, res: 0.0, sup: 0.0 }
    }
}

/// Total-least-squares line through `pts`, trimming what bites inward.
pub fn fit_line(pts: &[Pt], trim: bool) -> Option<Line> {
    let mut pts = pts.to_vec();
    let mut out = None;
    for _ in 0..(if trim { 4 } else { 1 }) {
        if pts.len() < 6 {
            return None;
        }
        let n = pts.len() as f64;
        let cx = pts.iter().map(|p| p.0).sum::<f64>() / n;
        let cy = pts.iter().map(|p| p.1).sum::<f64>() / n;
        let sxx: f64 = pts.iter().map(|p| (p.0 - cx).powi(2)).sum();
        let syy: f64 = pts.iter().map(|p| (p.1 - cy).powi(2)).sum();
        let sxy: f64 = pts.iter().map(|p| (p.0 - cx) * (p.1 - cy)).sum();
        let a = 0.5 * (2.0 * sxy).atan2(sxx - syy);
        let (dx, dy) = (a.cos(), a.sin());
        let res: Vec<f64> = pts.iter().map(|p| (-(p.0 - cx) * dy + (p.1 - cy) * dx).abs()).collect();
        let mut sr = res.clone();
        sr.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = sr[pts.len() / 2];
        out = Some(Line { cx, cy, dx, dy, res: med, sup: 0.0 });
        let lim = (3.0 * med * 1.4826).max(1.0);
        let kept: Vec<Pt> = pts.iter().zip(&res).filter(|(_, &r)| r <= lim).map(|(p, _)| *p).collect();
        if !trim || kept.len() == pts.len() {
            break;
        }
        pts = kept;
    }
    out
}

/// Intersection of two lines, or None when they are (nearly) parallel.
pub fn cross(l1: &Line, l2: &Line) -> Option<Pt> {
    let den = l1.dx * l2.dy - l1.dy * l2.dx;
    if den.abs() < 1e-9 {
        return None;
    }
    let t = ((l2.cx - l1.cx) * l2.dy - (l2.cy - l1.cy) * l2.dx) / den;
    Some((l1.cx + t * l1.dx, l1.cy + t * l1.dy))
}

/// Angle between two lines, 0-90 degrees.
pub fn line_angle(l1: &Line, l2: &Line) -> f64 {
    let c = (l1.dx * l2.dx + l1.dy * l2.dy).abs();
    c.min(1.0).acos().to_degrees()
}

const EDGE_BAND: f64 = 1.5;
const EDGE_OUT: f64 = 3.0;
const EDGE_PENALTY: f64 = 1.0;

/// The page's true edge among the side's outermost points (a support line).
fn edge_line(pts: &[Pt], p: Pt, q: Pt) -> Option<Line> {
    let l = dist(p, q);
    if l < 8.0 || pts.len() < 12 {
        return None;
    }
    let (ux, uy) = ((q.0 - p.0) / l, (q.1 - p.1) / l);
    let (nx, ny) = (uy, -ux);
    let mut pts = pts.to_vec();
    // Python's sort is stable, like this one
    pts.sort_by(|a, b| {
        let ka = (a.0 - p.0) * ux + (a.1 - p.1) * uy;
        let kb = (b.0 - p.0) * ux + (b.1 - p.1) * uy;
        ka.partial_cmp(&kb).unwrap()
    });
    let n = pts.len();
    let mut best: Option<(f64, Pt, f64, f64)> = None;
    for stride in [n / 8, n / 5, n / 3, n / 2] {
        if stride < 3 {
            continue;
        }
        let step = (n / 40).max(1);
        let mut i = 0;
        while i < n - stride {
            let (a, b) = (pts[i], pts[i + stride]);
            let (lx, ly) = (b.0 - a.0, b.1 - a.1);
            let ll = lx.hypot(ly);
            if ll >= 1.0 {
                let (mut mx, mut my) = (ly / ll, -lx / ll);
                if mx * nx + my * ny < 0.0 {
                    mx = -mx;
                    my = -my;
                }
                let (mut on, mut out) = (0.0, 0.0);
                for pp in &pts {
                    let s = (pp.0 - a.0) * mx + (pp.1 - a.1) * my;
                    if s.abs() <= EDGE_BAND {
                        on += 1.0;
                    } else if s > EDGE_OUT {
                        out += 1.0;
                    }
                }
                let score = on - EDGE_PENALTY * out;
                if best.is_none() || score > best.unwrap().0 {
                    best = Some((score, a, mx, my));
                }
            }
            i += step;
        }
    }
    let (_, a, mx, my) = best?;
    let inl: Vec<Pt> = pts.iter().copied().filter(|pp| ((pp.0 - a.0) * mx + (pp.1 - a.1) * my).abs() <= EDGE_BAND).collect();
    let mut ln = fit_line(&inl, false)?;
    ln.sup = inl.len() as f64 / n as f64;
    Some(ln)
}

/// The four edge lines of one page blob: top, right, bottom, left.
pub struct Sides {
    pub top: Line,
    pub right: Line,
    pub bottom: Line,
    pub left: Line,
}

impl Sides {
    pub fn get(&self, s: &str) -> Line {
        match s {
            "top" => self.top,
            "right" => self.right,
            "bottom" => self.bottom,
            _ => self.left,
        }
    }
    pub fn set(&mut self, s: &str, l: Line) {
        match s {
            "top" => self.top = l,
            "right" => self.right = l,
            "bottom" => self.bottom = l,
            _ => self.left = l,
        }
    }
}

fn corners(b: &Blob) -> (Pt, Pt, Pt, Pt) {
    let pts: Vec<Pt> = b.xs.iter().zip(&b.ys).map(|(&x, &y)| (x, y)).collect();
    let tl = arg_ext(&pts, |p| p.0 + p.1, false);
    let br = arg_ext(&pts, |p| p.0 + p.1, true);
    let tr = arg_ext(&pts, |p| p.0 - p.1, true);
    let bl = arg_ext(&pts, |p| p.0 - p.1, false);
    (tl, tr, br, bl)
}

pub fn page_lines(b: &Blob, w: usize, h: Option<usize>) -> Option<Sides> {
    if b.xs.is_empty() {
        return None;
    }
    let (tl, tr, br, bl) = corners(b);
    let top = edge_line(&side_points(b, w, tl, tr, h), tl, tr)?;
    let right = edge_line(&side_points(b, w, tr, br, h), tr, br)?;
    let bottom = edge_line(&side_points(b, w, br, bl, h), br, bl)?;
    let left = edge_line(&side_points(b, w, bl, tl, h), bl, tl)?;
    Some(Sides { top, right, bottom, left })
}

const FOLD_ANGLE: f64 = 1.5;
const FOLD_STEP: f64 = 1.0;
const FOLD_GAIN: f64 = 0.6;

fn kink(pts: &[Pt], p: Pt, q: Pt, short: f64) -> Option<(f64, Pt)> {
    let l = dist(p, q);
    let (ux, uy) = ((q.0 - p.0) / l, (q.1 - p.1) / l);
    let mut tp: Vec<(f64, Pt)> = pts.iter().map(|pp| (((pp.0 - p.0) * ux + (pp.1 - p.1) * uy) / l, *pp)).collect();
    // sorted() on tuples: by t, then by the point
    tp.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if tp.len() < 30 {
        return None;
    }
    let all: Vec<Pt> = tp.iter().map(|x| x.1).collect();
    let one = fit_line(&all, false)?;
    let mut best: Option<(f64, f64, Line, Line)> = None;
    for k in (35..66).step_by(2) {
        let t = k as f64 / 100.0;
        let a: Vec<Pt> = tp.iter().filter(|x| x.0 < t).map(|x| x.1).collect();
        let b: Vec<Pt> = tp.iter().filter(|x| x.0 >= t).map(|x| x.1).collect();
        let (la, lb) = match (fit_line(&a, false), fit_line(&b, false)) {
            (Some(x), Some(y)) => (x, y),
            _ => continue,
        };
        let err = (la.res * a.len() as f64 + lb.res * b.len() as f64) / tp.len() as f64;
        if best.is_none() || err < best.unwrap().0 {
            best = Some((err, t, la, lb));
        }
    }
    let (err, t, la, lb) = best?;
    if err > FOLD_GAIN * one.res {
        return None;
    }
    let x = (p.0 + t * l * ux, p.1 + t * l * uy);
    let at = |ln: &Line| cross(ln, &Line::of(x.0, x.1, -uy, ux)).unwrap_or(x);
    let (pa, pb) = (at(&la), at(&lb));
    let step = dist(pa, pb);
    if line_angle(&la, &lb) < FOLD_ANGLE && step < FOLD_STEP / 100.0 * short {
        return None;
    }
    Some((t, ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0)))
}

/// One paper region that is really two pages: split it at the fold.
fn split_blob(b: &Blob, w: usize) -> Option<(Vec<usize>, Vec<usize>)> {
    let ln = page_lines(b, w, None)?;
    let tl = cross(&ln.top, &ln.left)?;
    let tr = cross(&ln.top, &ln.right)?;
    let br = cross(&ln.bottom, &ln.right)?;
    let bl = cross(&ln.bottom, &ln.left)?;
    let wd = (dist(tl, tr) + dist(bl, br)) / 2.0;
    let ht = (dist(tl, bl) + dist(tr, br)) / 2.0;
    let short = wd.min(ht);
    let sides = if ht >= wd { [(tl, bl), (tr, br)] } else { [(tl, tr), (bl, br)] };
    let mut folds = vec![];
    for (p, q) in sides {
        let (a, bb) = if (p, q) == (tl, bl) || (p, q) == (bl, br) { (q, p) } else { (p, q) };
        folds.push(kink(&side_points(b, w, a, bb, None), p, q, short)?);
    }
    let ((t1, f1), (t2, f2)) = (folds[0], folds[1]);
    if (t1 - t2).abs() > 0.15 {
        return None;
    }
    let (fx, fy) = (f2.0 - f1.0, f2.1 - f1.1);
    let (mut a, mut c) = (vec![], vec![]);
    for &i in &b.comp {
        let (x, y) = ((i % w) as f64, (i / w) as f64);
        if fx * (y - f1.1) - fy * (x - f1.0) < 0.0 {
            a.push(i);
        } else {
            c.push(i);
        }
    }
    Some((a, c))
}

const SPREAD_V: f64 = 12.0;
const SPREAD_SUPPORT: f64 = 0.35;

pub struct Spread {
    pub quads: [Quad; 2],
    pub horiz: bool,
    pub why: String,
}

/// Two facing pages of an open booklet, or the reason there are none.
pub fn detect_spread(pm: &Pm) -> Result<Spread, String> {
    let (w, h) = (pm.w, pm.h);
    let n = (w * h) as f64;
    let mut comps: Vec<Vec<usize>> = components(&pm.mask, w, h).into_iter().take(2).collect();
    if comps.is_empty() {
        return Err("no paper region".into());
    }
    let mut how = "two paper regions";
    let mut a = comps[0].len() as f64 / n;
    let mut b = if comps.len() > 1 { comps[1].len() as f64 / n } else { 0.0 };
    if b * 100.0 < SPREAD_MIN && a * 100.0 >= 2.0 * SPREAD_MIN {
        if let Some((x, y)) = split_blob(&Blob::new(comps[0].clone(), w), w) {
            let mut v = vec![x, y];
            v.sort_by(|p, q| q.len().cmp(&p.len()));
            comps = v;
            a = comps[0].len() as f64 / n;
            b = comps[1].len() as f64 / n;
            how = "one paper region folded in the middle";
        }
    }
    if comps.len() < 2 || b * 100.0 < SPREAD_MIN {
        return Err(format!("the second paper region covers {} of the frame, too little for a facing page", pc0(b)));
    }
    if b / a < SPREAD_BALANCE {
        return Err(format!(
            "the two paper regions differ too much in size ({} and {}) to be facing pages",
            pc0(a),
            pc0(b)
        ));
    }
    let blobs: Vec<Blob> = comps.iter().map(|c| Blob::new(c.clone(), w)).collect();
    let mut fits = vec![];
    for bl in &blobs {
        match page_lines(bl, w, None) {
            Some(f) => fits.push(Some(f)),
            None => return Err("a page edge could not be fitted".into()),
        }
    }
    let cen: Vec<Pt> = comps
        .iter()
        .map(|c| {
            let k = c.len() as f64;
            (c.iter().map(|&i| (i % w) as f64).sum::<f64>() / k, c.iter().map(|&i| (i / w) as f64).sum::<f64>() / k)
        })
        .collect();
    let horiz = (cen[1].0 - cen[0].0).abs() >= (cen[1].1 - cen[0].1).abs();
    let key = |i: usize| if horiz { cen[i].0 } else { cen[i].1 };
    let order: Vec<usize> = if key(1) < key(0) { vec![1, 0] } else { vec![0, 1] };
    let mut aa = fits[order[0]].take().unwrap();
    let mut bb = fits[order[1]].take().unwrap();
    let (ca, cb) = (&comps[order[0]], &comps[order[1]]);
    let mut fixed = vec![];
    for side in if horiz { ["top", "bottom"] } else { ["left", "right"] } {
        let (la, lb) = (aa.get(side), bb.get(side));
        if line_angle(&la, &lb) > SPREAD_V && la.sup.min(lb.sup) < SPREAD_SUPPORT {
            if la.sup < lb.sup {
                aa.set(side, lb);
            } else {
                bb.set(side, la);
            }
            fixed.push(side);
        }
    }
    let span = w.max(h) as f64;
    let fold = |ea: &str, eb: &str, ia: &str, ib: &str| -> Option<Pt> {
        let pa = cross(&aa.get(ea), &aa.get(ia))?;
        let pb = cross(&bb.get(eb), &bb.get(ib))?;
        let mid = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
        let v = cross(&aa.get(ea), &bb.get(eb));
        let gap = dist(pa, pb);
        match v {
            Some(v) if dist(v, mid) <= gap / 2.0 + 0.03 * span => Some(v),
            _ => Some(mid),
        }
    };
    let c = |s: &Sides, x: &str, y: &str| cross(&s.get(x), &s.get(y));
    let (qa, qb) = if horiz {
        let (f1, f2) = match (fold("top", "top", "right", "left"), fold("bottom", "bottom", "right", "left")) {
            (Some(x), Some(y)) => (x, y),
            _ => return Err("the fold could not be located".into()),
        };
        (
            [c(&aa, "top", "left"), Some(f1), Some(f2), c(&aa, "bottom", "left")],
            [Some(f1), c(&bb, "top", "right"), c(&bb, "bottom", "right"), Some(f2)],
        )
    } else {
        let (f1, f2) = match (fold("left", "left", "bottom", "top"), fold("right", "right", "bottom", "top")) {
            (Some(x), Some(y)) => (x, y),
            _ => return Err("the fold could not be located".into()),
        };
        (
            [c(&aa, "left", "top"), c(&aa, "right", "top"), Some(f2), Some(f1)],
            [Some(f1), Some(f2), c(&bb, "right", "bottom"), c(&bb, "left", "bottom")],
        )
    };
    let mut quads = vec![];
    for (q, comp) in [(qa, ca), (qb, cb)] {
        if q.iter().any(|p| p.is_none()) {
            return Err("a page corner could not be located".into());
        }
        let q: Quad = q.map(|p| p.unwrap());
        for i in 0..4 {
            let ang = angle(q[(i + 3) % 4], q[i], q[(i + 1) % 4]);
            if !(45.0..=135.0).contains(&ang) {
                return Err(format!("page corner angle {:.0}° is not a page corner", ang));
            }
        }
        let area = shoelace(&q);
        let fill = if area > 0.0 { comp.len() as f64 / area } else { 0.0 };
        if !(SPREAD_FILL..=1.1).contains(&fill) {
            return Err(format!("a page region fills {} of its fitted outline, not a page", pc0(fill)));
        }
        let (s01, s32, s03, s12) = (dist(q[0], q[1]), dist(q[3], q[2]), dist(q[0], q[3]), dist(q[1], q[2]));
        if s01.max(s32) > 1.8 * s01.min(s32) || s03.max(s12) > 1.8 * s03.min(s12) {
            return Err("opposite page edges differ too much to be one flat page".into());
        }
        quads.push(q);
    }
    let (sx, sy) = (pm.w0 as f64 / w as f64, pm.h0 as f64 / h as f64);
    let full = |q: &Quad| q.map(|p| (p.0 * sx, p.1 * sy));
    let mut why = format!("{}, {} + {} of the frame", how, pc0(a), pc0(b));
    if !fixed.is_empty() {
        why += &format!("; the {} edge taken from the facing page", fixed.join(" and "));
    }
    Ok(Spread { quads: [full(&quads[0]), full(&quads[1])], horiz, why })
}

/// (horizontal, vertical) ink in long runs: which way the lines of text go.
pub fn ink_runs(img: &Img, side: usize, thr: f64) -> (usize, usize) {
    let (w0, h0) = (img.w, img.h);
    let (w, h) = long_side(w0, h0, side);
    let g = img.resize_auto(w, h).gray();
    let closed = ops::morph_p(&g, &ops::octagon(3), true, 1);
    let closed = ops::morph_p(&closed, &ops::octagon(3), false, 1);
    let bg = ops::blur(&closed, 2.0);
    let div = g.zip(&bg, divide);
    let raw = ops::threshold(&div, thr).bytes();
    let minlen = 8.max(ops::py_round(0.025 * w.max(h) as f64) as usize);
    let runs = |n_lines: usize, n_len: usize, at: &dyn Fn(usize, usize) -> usize| -> usize {
        let gap = 2;
        let mut tot = 0;
        for a in 0..n_lines {
            let (mut cur, mut hole) = (0, 0);
            for b in 0..n_len {
                if raw[at(a, b)] < 128 {
                    cur = if cur > 0 { cur + hole + 1 } else { 1 };
                    hole = 0;
                } else if cur > 0 {
                    hole += 1;
                    if hole > gap {
                        if cur >= minlen {
                            tot += cur;
                        }
                        cur = 0;
                        hole = 0;
                    }
                }
            }
            if cur >= minlen {
                tot += cur;
            }
        }
        tot
    };
    (runs(h, w, &|y, x| y * w + x), runs(w, h, &|x, y| y * w + x))
}

/// ImageMagick's Divide, dst / src, for opaque pixels.
pub fn divide(d: f32, s: f32) -> f32 {
    const E: f32 = 1.0e-12;
    if s.abs() < E {
        if d.abs() < E {
            0.0
        } else {
            1.0
        }
    } else {
        (d / s).min(1.0)
    }
}

/// The size of a copy `side` px on its long side.
pub fn long_side(w0: usize, h0: usize, side: usize) -> (usize, usize) {
    if w0 >= h0 {
        (side, (ops::py_round(side as f64 * h0 as f64 / w0 as f64)).max(1) as usize)
    } else {
        ((ops::py_round(side as f64 * w0 as f64 / h0 as f64)).max(1) as usize, side)
    }
}

pub const PHOTO_CELL: usize = 8;
const PHOTO_DARK: f64 = 0.78;
const PHOTO_CELL_MIN: f64 = 0.45;
const PHOTO_AREA: (f64, f64) = (1.0, 30.0);
const PHOTO_FILL: f64 = 0.4;
const PHOTO_TRIES: usize = 4;
const PHOTO_REACH: usize = 6;

pub struct Cells {
    pub dark: Vec<u8>,
    pub cw: usize,
    pub ch: usize,
    pub w: usize,
    pub w0: usize,
}

/// Cells of a downscaled copy mostly darker than the paper near them.
pub fn dark_cells(img: &Img, side: usize) -> Option<Cells> {
    let (w0, h0) = (img.w, img.h);
    let (w, h) = long_side(w0, h0, side);
    let (v, _, _) = raw_rgb(img, w, h);
    let c = PHOTO_CELL;
    let (cw, ch) = (w / c, h / c);
    if cw < 4 || ch < 4 {
        return None;
    }
    let mut top = vec![0u8; cw * ch];
    for cy in 0..ch {
        for cx in 0..cw {
            let mut vals: Vec<u8> = (cy * c..cy * c + c).flat_map(|y| (cx * c..cx * c + c).map(move |x| (y, x))).map(|(y, x)| v[y * w + x]).collect();
            vals.sort_unstable();
            top[cy * cw + cx] = vals[(0.9 * (vals.len() - 1) as f64) as usize];
        }
    }
    let r = PHOTO_REACH;
    let filt = |src: &[u8], mx: bool| -> Vec<u8> {
        let pick = |a: u8, b: u8| if mx { a.max(b) } else { a.min(b) };
        let mut tmp = vec![0u8; cw * ch];
        for cy in 0..ch {
            for cx in 0..cw {
                let mut m = src[cy * cw + cx.saturating_sub(r)];
                for xx in cx.saturating_sub(r)..(cx + r + 1).min(cw) {
                    m = pick(m, src[cy * cw + xx]);
                }
                tmp[cy * cw + cx] = m;
            }
        }
        let mut out = vec![0u8; cw * ch];
        for cy in 0..ch {
            for cx in 0..cw {
                let mut m = tmp[cy.saturating_sub(r) * cw + cx];
                for yy in cy.saturating_sub(r)..(cy + r + 1).min(ch) {
                    m = pick(m, tmp[yy * cw + cx]);
                }
                out[cy * cw + cx] = m;
            }
        }
        out
    };
    let bg = filt(&filt(&top, true), false);
    let mut dark = vec![0u8; cw * ch];
    for cy in 0..ch {
        for cx in 0..cw {
            let lim = PHOTO_DARK * bg[cy * cw + cx] as f64;
            let mut cnt = 0;
            for y in cy * c..cy * c + c {
                for x in cx * c..cx * c + c {
                    if (v[y * w + x] as f64) < lim {
                        cnt += 1;
                    }
                }
            }
            if cnt as f64 >= PHOTO_CELL_MIN * (c * c) as f64 {
                dark[cy * cw + cx] = 1;
            }
        }
    }
    Some(Cells { dark, cw, ch, w, w0 })
}

/// Bounding box (x, y, w, h) of a face photo on the page, or None.
pub fn find_photo_block(img: &Img, area_min: Option<f64>, area_max: Option<f64>, fill: Option<f64>, aspect: Option<(f64, f64)>) -> Option<(i64, i64, i64, i64)> {
    let cells = dark_cells(img, 400)?;
    let (cw, ch) = (cells.cw, cells.ch);
    let c = PHOTO_CELL as f64;
    let asp = aspect.unwrap_or((0.4, 2.5));
    let mut got = None;
    for comp in components(&cells.dark, cw, ch).into_iter().take(PHOTO_TRIES) {
        let xs: Vec<usize> = comp.iter().map(|&i| i % cw).collect();
        let ys: Vec<usize> = comp.iter().map(|&i| i / cw).collect();
        let (x0, x1) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
        let (y0, y1) = (*ys.iter().min().unwrap(), *ys.iter().max().unwrap());
        let (bw, bh) = ((x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64);
        let share = 100.0 * bw * bh / (cw * ch) as f64;
        if area_min.unwrap_or(PHOTO_AREA.0) <= share
            && share <= area_max.unwrap_or(PHOTO_AREA.1)
            && comp.len() as f64 >= fill.unwrap_or(PHOTO_FILL) * bw * bh
            && asp.0 <= bw / bh
            && bw / bh <= asp.1
        {
            got = Some((x0 as f64, x1 as f64, y0 as f64, y1 as f64, bw, bh));
            break;
        }
    }
    let (mnx, mxx, mny, mxy, bw, bh) = got?;
    let x0 = (mnx - 0.15 * bw).max(0.0);
    let y0 = (mny - 0.05 * bh).max(0.0);
    let x1 = (cw as f64).min(mxx + 1.0 + 0.15 * bw);
    let y1 = (ch as f64).min(mxy + 1.0 + 0.15 * bh);
    let s = cells.w0 as f64 / cells.w as f64;
    Some((
        ops::py_round(x0 * c * s),
        ops::py_round(y0 * c * s),
        ops::py_round((x1 - x0) * c * s),
        ops::py_round((y1 - y0) * c * s),
    ))
}

pub const SPREAD_PHOTO_MM: (f64, f64) = (38.0, 48.0);
pub const SPREAD_PHOTO_DARK: f64 = 0.25;
const SPREAD_PHOTO_PLACE: (f64, f64, f64, f64) = (0.03, 0.37, 0.58, 0.98);
pub const CARD_FACE_DARK: f64 = 0.30;
pub const CARD_FACE_RUN: f64 = 0.20;

/// (dark share, column share) of a place in a dark-cell map.
pub fn place(cells: &Cells, x0: f64, x1: f64, y0: f64, y1: f64) -> (f64, f64) {
    let (cw, ch) = (cells.cw, cells.ch);
    let xa = (x0 * cw as f64) as usize;
    let xb = (xa + 1).max((x1 * cw as f64) as usize);
    let ya = (y0 * ch as f64) as usize;
    let yb = (ya + 1).max((y1 * ch as f64) as usize);
    let cols: Vec<usize> = (xa..xb.min(cw)).collect();
    if cols.is_empty() {
        return (0.0, 0.0);
    }
    let rows_: Vec<usize> = (ya..yb.min(ch)).collect();
    let n: usize = rows_.iter().map(|&cy| cols.iter().map(|&cx| cells.dark[cy * cw + cx] as usize).sum::<usize>()).sum();
    let mut runs = 0;
    for &cx in &cols {
        let (mut best, mut cur) = (0, 0);
        for &cy in &rows_ {
            cur = if cells.dark[cy * cw + cx] != 0 { cur + 1 } else { 0 };
            best = best.max(cur);
        }
        if best as f64 >= 0.5 * (yb - ya) as f64 {
            runs += 1;
        }
    }
    (
        n as f64 / (cols.len() * 1.max(yb.min(ch) as isize - ya as isize) as usize) as f64,
        runs as f64 / cols.len() as f64,
    )
}

/// Degrees to turn a rectified spread so its text reads upright.
pub fn spread_turn(img: &Img, horiz: bool, report: &mut Vec<String>) -> i32 {
    let (hr, vr) = ink_runs(img, 600, 75.0);
    let ratio = hr as f64 / vr.max(1) as f64;
    let cands: [i32; 2] = if ratio >= 1.6 {
        [0, 180]
    } else if ratio <= 1.0 / 1.6 {
        [90, 270]
    } else {
        report.push(format!(
            "spread: text direction unclear (runs across/along {:.2}) — left as photographed; pass --rotate",
            ratio
        ));
        return 0;
    };
    let lines = if cands[0] == 0 { "across" } else { "up and down" };
    let (iw, ih) = (img.w as f64, img.h as f64);
    let mut scored = vec![];
    for turn in cands {
        if horiz == (turn == 0 || turn == 180) {
            continue;
        }
        // -resize 400x400: fitted inside, aspect kept
        let s = (400.0 / iw).min(400.0 / ih);
        let (sw, sh) = if 400.0 / iw <= 400.0 / ih {
            (400, (ih * s + 0.5).floor().max(1.0) as usize)
        } else {
            ((iw * s + 0.5).floor().max(1.0) as usize, 400)
        };
        let mut small = img.resize_auto(sw, sh).rotate(turn);
        small.q8();
        if let Some(cells) = dark_cells(&small, 200) {
            let (d, r) = place(&cells, SPREAD_PHOTO_PLACE.0, SPREAD_PHOTO_PLACE.1, SPREAD_PHOTO_PLACE.2, SPREAD_PHOTO_PLACE.3);
            scored.push((d, r, turn));
        }
    }
    let mut pick = None;
    if scored.len() == 2 {
        let mut s = scored.clone();
        s.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let ((d1, _, t1), (d2, _, _)) = ((s[0].0, s[0].1, s[0].2), (s[1].0, s[1].1, s[1].2));
        if d1 >= SPREAD_PHOTO_DARK && d1 >= 3.0 * d2 + 0.02 {
            pick = Some(t1);
        }
    }
    for &(d, r, t) in &scored {
        if pick.is_none() && d >= CARD_FACE_DARK && r >= CARD_FACE_RUN {
            pick = Some(t);
        }
    }
    if let Some(turn) = pick {
        report.push(format!(
            "spread: text runs {} (ratio {:.2}); the face photo is in its place, left on the lower page, when turned {}°",
            lines, ratio, turn
        ));
        return turn;
    }
    if let Some(bx) = find_photo_block(img, None, None, None, None) {
        let (cx, cy) = (bx.0 as f64 + bx.2 as f64 / 2.0, bx.1 as f64 + bx.3 as f64 / 2.0);
        for turn in cands {
            let (mut x, mut ww) = match turn {
                0 => (cx, iw),
                180 => (iw - cx, iw),
                90 => (ih - cy, ih),
                _ => (cy, ih),
            };
            let side_by_side = horiz == (turn == 0 || turn == 180);
            if side_by_side {
                ww /= 2.0;
                x = x.rem_euclid(ww);
            }
            if x < ww / 2.0 {
                report.push(format!(
                    "spread: text runs {} (ratio {:.2}); the photo sits left of its page when turned {}°",
                    lines, ratio, turn
                ));
                return turn;
            }
        }
    }
    let turn = cands[0];
    report.push(format!(
        "spread: text runs {} (ratio {:.2}); no face photo to tell up from down — turned {}°, pass --rotate if it came out upside down",
        lines, ratio, turn
    ));
    turn
}

// -------------------------------------------------------------------- cards

pub const CARD_MM: (f64, f64) = (85.60, 53.98);
pub const CARD_RADIUS_MM: f64 = 3.18;
pub const CARD_ASPECT: (f64, f64) = (1.50, 1.68);
const CARD_MIN: f64 = 2.0;
const CARD_FILL: f64 = 0.6;
pub const CARD_OF_SPREAD: f64 = 0.7;
const CARD_GAP: f64 = 4.0;

fn card_quad(b: &Blob, w: usize, h: usize) -> Option<(Quad, f64)> {
    let lines = page_lines(b, w, Some(h)).or_else(|| page_lines(b, w, None))?;
    let q = [
        cross(&lines.top, &lines.left)?,
        cross(&lines.top, &lines.right)?,
        cross(&lines.bottom, &lines.right)?,
        cross(&lines.bottom, &lines.left)?,
    ];
    for i in 0..4 {
        if !(60.0..=120.0).contains(&angle(q[(i + 3) % 4], q[i], q[(i + 1) % 4])) {
            return None;
        }
    }
    let (top, bot, lef, rig) = (dist(q[0], q[1]), dist(q[3], q[2]), dist(q[0], q[3]), dist(q[1], q[2]));
    if top.max(bot) > 1.35 * top.min(bot) || lef.max(rig) > 1.35 * lef.min(rig) {
        return None;
    }
    let area = shoelace(&q);
    if area <= 0.0 || !(CARD_FILL..=1.1).contains(&(b.len() as f64 / area)) {
        return None;
    }
    let (wd, ht) = ((top + bot) / 2.0, (lef + rig) / 2.0);
    let aspect = wd.max(ht) / wd.min(ht);
    if !(CARD_ASPECT.0..=CARD_ASPECT.1).contains(&aspect) {
        return None;
    }
    Some((q, aspect))
}

/// One or two ID-1 cards in the frame, or why not.
pub fn detect_cards(vc: &Vc) -> Result<(Vec<Quad>, String), String> {
    let (w, h) = (vc.w, vc.h);
    let n = (w * h) as f64;
    let mut whys = vec![];
    for mode in [Mode::Paper, Mode::Tinted, Mode::Otsu] {
        let pm = paper_mask(vc, mode);
        let mut found = vec![];
        for comp in components(&pm.mask, w, h).into_iter().take(4) {
            let share = 100.0 * comp.len() as f64 / n;
            if share < CARD_MIN {
                break;
            }
            if let Some((q, a)) = card_quad(&Blob::new(comp, w), w, h) {
                found.push((q, a, share));
            }
        }
        if found.is_empty() {
            whys.push(format!("{}: no card-shaped region", mode.name()));
            continue;
        }
        found.truncate(2);
        if found.len() == 2 {
            let (a1, a2) = (shoelace(&found[0].0), shoelace(&found[1].0));
            let short = (a1.min(a2) / CARD_ASPECT.0).sqrt();
            let mut gap = f64::MAX;
            for p in found[0].0 {
                for r in found[1].0 {
                    gap = gap.min(dist(p, r));
                }
            }
            if a1.min(a2) < 0.6 * a1.max(a2) {
                found.truncate(1);
            } else if gap < CARD_GAP / 100.0 * short {
                return Err(format!("{}: two card-shaped regions touching — a booklet, not two cards", mode.name()));
            }
        }
        let (sx, sy) = (vc.w0 as f64 / w as f64, vc.h0 as f64 / h as f64);
        let quads = found.iter().map(|f| f.0.map(|p| (p.0 * sx, p.1 * sy))).collect();
        let why = found.iter().map(|f| format!("{:.0}% of the frame, sides {:.2}:1", f.2, f.1)).collect::<Vec<_>>().join(", ");
        return Ok((quads, format!("{}, {} paper mask", why, mode.name())));
    }
    Err(whys.join("; "))
}

// ------------------------------------------------------------ edge finding

const EDGE_SIDE: usize = 600;
const EDGE_LINES: usize = 16;
const EDGE_SUPPORT: f64 = 0.45;
const EDGE_SHEET: f64 = 0.60;

pub struct EdgeMap {
    pub e: Vec<u8>,
    pub w: usize,
    pub h: usize,
    pub w0: usize,
    pub h0: usize,
    pub hough: Vec<(f64, f64, f64, f64, f64)>,
    pub g: Vec<u8>,
}

/// The frame's edges at the analysis scale: Canny of the brightness and of
/// the saturation, the Hough lines when asked, the grey copy.
pub fn edge_map(img: &dyn Pix, hough: bool) -> EdgeMap {
    let (w0, h0) = img.dims();
    let (w, h) = long_side(w0, h0, EDGE_SIDE);
    let s = img::resize_any(img, w, h);
    let e1 = ops::canny(&s.gray(), 2.0, 0.10, 0.30);
    let mut sat = s.hsl_saturation();
    sat.q8();
    let e2 = ops::canny(&sat, 2.0, 0.10, 0.30);
    let edge: Vec<u8> = e1.iter().zip(&e2).map(|(a, b)| a | b).collect();
    let lines = if hough {
        let thr = 20.max(ops::py_round(w.min(h) as f64 * 0.12) as usize);
        ops::hough_lines(&edge, w, h, 15, 15, thr)
    } else {
        vec![]
    };
    let e = ops::morph_k(&edge, w, h, &ops::square(2), true).iter().map(|&v| if v != 0 { 255 } else { 0 }).collect();
    EdgeMap { e, w, h, w0, h0, hough: lines, g: raw_gray(img, w, h) }
}

/// The share of the straight segment a-b lying on an edge.
fn seg_support(em: &EdgeMap, a: Pt, b: Pt) -> f64 {
    let n = 10.max(dist(a, b) as usize);
    let mut hit = 0;
    for k in 0..n {
        let t = (k as f64 + 0.5) / n as f64;
        let x = ops::py_round(a.0 + (b.0 - a.0) * t);
        let y = ops::py_round(a.1 + (b.1 - a.1) * t);
        if x >= 0 && y >= 0 && (x as usize) < em.w && (y as usize) < em.h && em.e[y as usize * em.w + x as usize] > 128 {
            hit += 1;
        }
    }
    hit as f64 / n as f64
}

pub const CARD_ALONE: f64 = 0.5;
const CARD_RUN_ON: (f64, f64) = (0.03, 0.18);
const CARD_EDGE_OFF: f64 = 4.0;

/// How far a card's boundary carries on past its corners.
pub fn card_run_on(q: &Quad, g: &[u8], w: usize, h: usize) -> f64 {
    let off = CARD_EDGE_OFF;
    let step = |p: Pt, ux: f64, uy: f64, nx: f64, ny: f64, t0: f64, t1: f64| -> f64 {
        let mut vals = vec![];
        let n = 8.max((t1 - t0).abs() as usize);
        for k in 0..n {
            let t = t0 + (t1 - t0) * (k as f64 + 0.5) / n as f64;
            let (x, y) = (p.0 + ux * t, p.1 + uy * t);
            let (xa, ya) = (ops::py_round(x + nx * off), ops::py_round(y + ny * off));
            let (xb, yb) = (ops::py_round(x - nx * off), ops::py_round(y - ny * off));
            let ok = |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h;
            if ok(xa, ya) && ok(xb, yb) {
                vals.push(g[ya as usize * w + xa as usize] as i32 - g[yb as usize * w + xb as usize] as i32);
            }
        }
        vals.sort();
        if vals.is_empty() {
            0.0
        } else {
            vals[vals.len() / 2] as f64
        }
    };
    let mut worst: f64 = 0.0;
    let (g0, g1) = CARD_RUN_ON;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let l = dist(a, b);
        if l < 1.0 {
            continue;
        }
        let (ux, uy) = ((b.0 - a.0) / l, (b.1 - a.1) / l);
        let (nx, ny) = (uy, -ux);
        let side = step(a, ux, uy, nx, ny, 0.2 * l, 0.8 * l);
        if side.abs() < 8.0 {
            continue;
        }
        for (t0, t1) in [(-g1 * l, -g0 * l), ((1.0 + g0) * l, (1.0 + g1) * l)] {
            worst = worst.max((step(a, ux, uy, nx, ny, t0, t1) / side).min(1.0));
        }
    }
    worst
}

/// Keep only the cards that are things on their own.
pub fn cards_alone(img: &dyn Pix, quads: &[Quad], report: &mut Vec<String>) -> Vec<Quad> {
    let em = edge_map(img, false);
    let mut kept = vec![];
    for q in quads {
        let qs = q.map(|p| (p.0 * em.w as f64 / em.w0 as f64, p.1 * em.h as f64 / em.h0 as f64));
        let runs = card_run_on(&qs, &em.g, em.w, em.h);
        if runs >= CARD_ALONE {
            report.push(format!(
                "card-shaped region refused: a side runs on past its corner ({} of its contrast) — part of something larger",
                pc0(runs)
            ));
        } else {
            kept.push(*q);
        }
    }
    kept
}

struct HLine {
    l: Line,
    cnt: f64,
    pre: Vec<u32>,
    span: i64,
}

impl HLine {
    fn support(&self, a: Pt, b: Pt) -> f64 {
        let l = &self.l;
        let mut ta = (a.0 - l.cx) * l.dx + (a.1 - l.cy) * l.dy;
        let mut tb = (b.0 - l.cx) * l.dx + (b.1 - l.cy) * l.dy;
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        let s = self.span;
        let i0 = (ops::py_round(ta) + s).clamp(0, 2 * s + 1);
        let i1 = (ops::py_round(tb) + s + 1).clamp(0, 2 * s + 1);
        (self.pre[i1 as usize] as f64 - self.pre[i0 as usize] as f64) / 1.max(i1 - i0) as f64
    }
}

struct Cand {
    quad: Quad,
    score: f64,
    minsup: f64,
    share: f64,
}

/// `%g` would have written this in exponent form, which the script's
/// pattern for a Hough line does not read: such a line is dropped.
fn g_exponent(v: f64) -> bool {
    v != 0.0 && (v.abs() < 1e-4 || v.abs() >= 1e6)
}

fn edge_quads(img: &dyn Pix) -> (Vec<Cand>, Vec<HLine>, EdgeMap) {
    let em = edge_map(img, true);
    let (w, h) = (em.w, em.h);
    let mut lines = vec![];
    for &(x1, y1, x2, y2, cnt) in &em.hough {
        if [x1, y1, x2, y2].iter().any(|&v| g_exponent(v)) {
            continue;
        }
        let n = (x2 - x1).hypot(y2 - y1);
        if n < 1.0 {
            continue;
        }
        let (dx, dy) = ((x2 - x1) / n, (y2 - y1) / n);
        let span = ((w as f64).hypot(h as f64)) as i64 + 2;
        let mut pre = vec![0u32; (2 * span + 2) as usize];
        for k in -span..=span {
            let x = ops::py_round(x1 + dx * k as f64);
            let y = ops::py_round(y1 + dy * k as f64);
            let hit = (x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && em.e[y as usize * w + x as usize] > 128) as u32;
            pre[(k + span + 1) as usize] = pre[(k + span) as usize] + hit;
        }
        lines.push(HLine { l: Line::of(x1, y1, dx, dy), cnt, pre, span });
    }
    let pick = |hz: bool| -> Vec<usize> {
        let mut v: Vec<usize> = (0..lines.len()).filter(|&i| (lines[i].l.dx.abs() >= lines[i].l.dy.abs()) == hz).collect();
        v.sort_by(|&a, &b| lines[b].cnt.partial_cmp(&lines[a].cnt).unwrap());
        v.truncate(EDGE_LINES);
        v
    };
    let (horiz, vert) = (pick(true), pick(false));
    let mut cands = vec![];
    let (wf, hf) = (w as f64, h as f64);
    for (i, &t) in horiz.iter().enumerate() {
        for &b in &horiz[i + 1..] {
            for (j, &le) in vert.iter().enumerate() {
                for &r in &vert[j + 1..] {
                    let (lt, lb, ll, lr) = (&lines[t], &lines[b], &lines[le], &lines[r]);
                    let q = match (cross(&lt.l, &ll.l), cross(&lt.l, &lr.l), cross(&lb.l, &lr.l), cross(&lb.l, &ll.l)) {
                        (Some(a), Some(bb), Some(c), Some(d)) => [a, bb, c, d],
                        _ => continue,
                    };
                    let cx = q.iter().map(|p| p.0).sum::<f64>() / 4.0;
                    let cy = q.iter().map(|p| p.1).sum::<f64>() / 4.0;
                    if q.iter().any(|p| p.0 < -0.05 * wf || p.0 > 1.05 * wf || p.1 < -0.05 * hf || p.1 > 1.05 * hf) {
                        continue;
                    }
                    let area = shoelace(&q);
                    if area < 0.05 * wf * hf {
                        continue;
                    }
                    if (0..4).any(|k| !(50.0..=130.0).contains(&angle(q[(k + 3) % 4], q[k], q[(k + 1) % 4]))) {
                        continue;
                    }
                    let sup = [lt.support(q[0], q[1]), lr.support(q[1], q[2]), lb.support(q[2], q[3]), ll.support(q[3], q[0])];
                    let minsup = sup.iter().cloned().fold(f64::MAX, f64::min);
                    if minsup < EDGE_SUPPORT {
                        continue;
                    }
                    let mut order: Vec<usize> = (0..4).collect();
                    order.sort_by(|&a, &b| {
                        (q[a].1 - cy).atan2(q[a].0 - cx).partial_cmp(&(q[b].1 - cy).atan2(q[b].0 - cx)).unwrap()
                    });
                    let qq: Vec<Pt> = order.iter().map(|&k| q[k]).collect();
                    let k0 = arg_min_idx(&qq);
                    let quad = [qq[k0], qq[(k0 + 1) % 4], qq[(k0 + 2) % 4], qq[(k0 + 3) % 4]];
                    let share = area / (wf * hf);
                    cands.push(Cand { quad, minsup, score: sup.iter().sum::<f64>() / 4.0 * share.sqrt(), share });
                }
            }
        }
    }
    cands.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    (cands, lines, em)
}

fn arg_min_idx(q: &[Pt]) -> usize {
    let mut k0 = 0;
    for k in 1..q.len() {
        if q[k].0 + q[k].1 < q[k0].0 + q[k0].1 {
            k0 = k;
        }
    }
    k0
}

fn quad_sides(q: &Quad) -> (f64, f64) {
    ((dist(q[0], q[1]) + dist(q[3], q[2])) / 2.0, (dist(q[0], q[3]) + dist(q[1], q[2])) / 2.0)
}

const FOLD_SUPPORT: f64 = 0.6;
const FOLD_MARGIN: f64 = 0.25;
const FOLD_SPREAD: f64 = 0.03;
const EDGE_SHEET_ASPECT: (f64, f64) = (1.25, 1.6);
const EDGE_SHEET_MAX: f64 = 0.85;
const EDGE_BRIGHT_FRAME: f64 = 0.7;
const EDGE_GROWTH: f64 = 1.3;

fn edge_fold(q: &Quad, lines: &[HLine]) -> Option<(Pt, Pt, bool)> {
    let (wd, ht) = quad_sides(q);
    let [tl, tr, br, bl] = *q;
    let (la, lb, horiz, mut rf) = if ht >= wd {
        ((tl, bl), (tr, br), false, Line::of(tl.0, tl.1, tr.0 - tl.0, tr.1 - tl.1))
    } else {
        ((tl, tr), (bl, br), true, Line::of(tl.0, tl.1, bl.0 - tl.0, bl.1 - tl.1))
    };
    let n = rf.dx.hypot(rf.dy);
    rf.dx /= n;
    rf.dy /= n;
    let mut strong = vec![];
    for ln in lines {
        if line_angle(&ln.l, &rf) > 12.0 {
            continue;
        }
        let mut pts = vec![];
        let mut ts = vec![];
        for (p, qq) in [la, lb] {
            let l = dist(p, qq);
            let side = Line::of(p.0, p.1, (qq.0 - p.0) / l, (qq.1 - p.1) / l);
            let c = match cross(&ln.l, &side) {
                Some(c) => c,
                None => break,
            };
            let t = ((c.0 - p.0) * side.dx + (c.1 - p.1) * side.dy) / l;
            if !(0.35..=0.65).contains(&t) {
                break;
            }
            pts.push(c);
            ts.push(t);
        }
        if pts.len() != 2 {
            continue;
        }
        let sup = ln.support(pts[0], pts[1]);
        if sup >= FOLD_SUPPORT {
            strong.push((sup, (ts[0] + ts[1]) / 2.0, pts[0], pts[1]));
        }
    }
    if strong.is_empty() {
        return None;
    }
    strong.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let best = strong[0];
    let strong: Vec<_> = strong.into_iter().filter(|x| x.0 >= best.0 - FOLD_MARGIN).collect();
    if strong.len() > 3 {
        return None;
    }
    if strong.iter().any(|x| (x.1 - best.1).abs() > FOLD_SPREAD) {
        return None;
    }
    Some((best.2, best.3, horiz))
}

fn inside(p: Pt, q: &Quad, slack: f64) -> bool {
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let cr = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if cr < -slack * (b.0 - a.0).hypot(b.1 - a.1) {
            return false;
        }
    }
    true
}

pub enum EdgeDoc {
    Cards(Vec<Quad>, String),
    Spread(Spread),
    Sheet(Quad, String),
}

/// What the edges say the document is, when brightness said nothing useful.
pub fn edge_document(img: &dyn Pix, bright: Option<Quad>, o: &Opts, report: &mut Vec<String>) -> Option<EdgeDoc> {
    let (cands, lines, em) = edge_quads(img);
    let best = cands.first()?;
    let q = best.quad;
    let (w, h) = (em.w, em.h);
    let (sx, sy) = (em.w0 as f64 / w as f64, em.h0 as f64 / h as f64);
    let full = |qq: &Quad| qq.map(|p| (p.0 * sx, p.1 * sy));
    let (wd, ht) = quad_sides(&q);
    let aspect = wd.max(ht) / 1e-6f64.max(wd.min(ht));
    let mut why = format!(
        "found by its edges: {} of the frame, sides {:.2}:1, outline {}+ edge",
        pc0(best.share),
        aspect,
        pc0(best.minsup)
    );
    if o.cards != "off" && (CARD_ASPECT.0..=CARD_ASPECT.1).contains(&aspect) {
        let runs = card_run_on(&q, &em.g, w, h);
        if runs < CARD_ALONE {
            return Some(EdgeDoc::Cards(vec![full(&q)], why));
        }
        report.push(format!(
            "card-shaped outline refused: a side runs on past its corner ({} of its contrast) — part of something larger",
            pc0(runs)
        ));
    }
    let mut only_spread = false;
    let mut edged_sheet = false;
    if let Some(bq) = bright {
        let bq = bq.map(|p| (p.0 / sx, p.1 / sy));
        let slack = 0.02 * w.max(h) as f64;
        let bsup = || {
            let mut s: Vec<f64> = (0..4).map(|i| seg_support(&em, bq[i], bq[(i + 1) % 4])).collect();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            s
        };
        if bq.iter().all(|&p| inside(p, &q, slack)) && shoelace(&q) >= EDGE_GROWTH * shoelace(&bq) {
            if bsup()[1] >= EDGE_SHEET {
                edged_sheet = true;
            }
            why += ", the brightness quad was a scrap inside it";
        } else if (q.iter().all(|&p| inside(p, &bq, slack)) && shoelace(&bq) >= EDGE_GROWTH * shoelace(&q))
            || shoelace(&bq) >= EDGE_BRIGHT_FRAME * (w * h) as f64
        {
            why += ", the brightness quad took the background with it";
            only_spread = bsup()[1] >= EDGE_SHEET;
        } else {
            return None;
        }
    }
    if edged_sheet {
        only_spread = true;
    }
    if o.spread != "off" {
        if let Some((f1, f2, horiz)) = edge_fold(&q, &lines) {
            let [tl, tr, br, bl] = q;
            let (qa, qb) = if !horiz { ([tl, tr, f2, f1], [f1, f2, br, bl]) } else { ([tl, f1, f2, bl], [f1, tr, br, f2]) };
            let ok = [qa, qb].iter().all(|pq| {
                let (pw, ph) = quad_sides(pq);
                (1.15..=1.75).contains(&(pw.max(ph) / 1e-6f64.max(pw.min(ph))))
            });
            if ok {
                return Some(EdgeDoc::Spread(Spread {
                    quads: [full(&qa), full(&qb)],
                    horiz,
                    why: why + ", a fold across the middle",
                }));
            }
        }
    }
    if !only_spread
        && best.minsup >= EDGE_SHEET
        && (0.15..=EDGE_SHEET_MAX).contains(&best.share)
        && (EDGE_SHEET_ASPECT.0..=EDGE_SHEET_ASPECT.1).contains(&aspect)
    {
        let runs = card_run_on(&q, &em.g, w, h);
        if runs < CARD_ALONE {
            return Some(EdgeDoc::Sheet(full(&q), why));
        }
        report.push(format!("edge outline refused as a sheet: a side runs on past its corner ({} of its contrast)", pc0(runs)));
    }
    None
}

// -------------------------------------------------------- the frame's border

fn smooth(v: &[f64], k: usize) -> Vec<f64> {
    let n = v.len();
    let half = k / 2;
    (0..n)
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(half), (i + half + 1).min(n));
            v[lo..hi].iter().sum::<f64>() / (hi - lo) as f64
        })
        .collect()
}

fn find_step(prof: &[f64], core: f64, outer_frac: f64, min_step: f64) -> Option<usize> {
    let n = prof.len();
    let band = 6.max((n as f64 * outer_frac) as usize);
    let (mut bi, mut bd) = (None, 0.0);
    for i in 3..band {
        if i + 2 >= n {
            break;
        }
        let d = (prof[i + 2] - prof[i - 3]).abs();
        if d > bd {
            bd = d;
            bi = Some(i);
        }
    }
    let bi = bi?;
    if bd < min_step {
        return None;
    }
    let outer = &prof[..bi];
    let mu = outer.iter().sum::<f64>() / outer.len() as f64;
    let var = outer.iter().map(|v| (v - mu).powi(2)).sum::<f64>() / outer.len() as f64;
    if (mu - core).abs() < min_step && var.sqrt() < min_step {
        return None;
    }
    Some(bi + 2)
}

/// Photo border to cut: (left, right, top, bottom, [left, right, top, bottom found]).
pub fn detect_border(img: &Img, outer_frac: f64, min_step: f64) -> (usize, usize, usize, usize, [bool; 4]) {
    let (w0, h0) = (img.w, img.h);
    let w = 420;
    let h = (ops::py_round(420.0 * h0 as f64 / w0 as f64)).max(1) as usize;
    let buf = raw_gray(img, w, h);
    let (x0, x1) = ((w as f64 * 0.20) as usize, (w as f64 * 0.80) as usize);
    let (y0, y1) = ((h as f64 * 0.20) as usize, (h as f64 * 0.80) as usize);
    let rws: Vec<f64> = (0..h).map(|y| buf[y * w + x0..y * w + x1].iter().map(|&v| v as f64).sum::<f64>() / (x1 - x0) as f64).collect();
    let cols: Vec<f64> = (0..w).map(|x| (y0..y1).map(|y| buf[y * w + x] as f64).sum::<f64>() / (y1 - y0) as f64).collect();
    let core = rws[y0..y1].iter().sum::<f64>() / 1.max(y1 - y0) as f64;
    let (rws, cols) = (smooth(&rws, 5), smooth(&cols, 5));
    let rev = |v: &[f64]| v.iter().rev().copied().collect::<Vec<f64>>();
    let top = find_step(&rws, core, outer_frac, min_step);
    let bottom = find_step(&rev(&rws), core, outer_frac, min_step);
    let left = find_step(&cols, core, outer_frac, min_step);
    let right = find_step(&rev(&cols), core, outer_frac, min_step);
    let (sx, sy) = (w0 as f64 / w as f64, h0 as f64 / h as f64);
    let r = |v: Option<usize>, s: f64| ops::py_round(v.unwrap_or(0) as f64 * s) as usize;
    (r(left, sx), r(right, sx), r(top, sy), r(bottom, sy), [left.is_some(), right.is_some(), top.is_some(), bottom.is_some()])
}

/// Bounding box of the ink, (x, y, w, h).
pub fn ink_bbox(img: &Img, thr: f64) -> Option<(usize, usize, usize, usize)> {
    let g = img.gray();
    let t = ops::pct_thr(thr);
    let m = g.map(|v| if v > t { 0.0 } else { 1.0 });
    let m = ops::morph_p(&m, &ops::octagon(1), false, 1);
    ops::trim_box(&m)
}

/// The skew `-threshold 60% -deskew 40%` reports.
pub fn deskew_angle(img: &Img) -> f64 {
    let g = ops::threshold(&img.gray(), 60.0);
    ops::deskew_angle(&[&g], ops::pct_thr(40.0))
}

