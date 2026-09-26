//! The ImageMagick operators a4norm uses, reproduced with ImageMagick's own
//! geometry: where a sample lands, how a resize weighs its neighbours, how
//! wide a Gaussian is, which pixels a kernel covers. One channel at a time,
//! values f32 in 0..1; what ImageMagick clamps between operators (Q16
//! without HDRI, the browser's build) is clamped here too.

#[cfg(feature = "par")]
use rayon::prelude::*;

/// One channel.
#[derive(Clone, Debug)]
pub struct Plane {
    pub w: usize,
    pub h: usize,
    pub d: Vec<f32>,
}

impl Plane {
    pub fn new(w: usize, h: usize) -> Self {
        Plane { w, h, d: vec![0.0; w * h] }
    }
    pub fn filled(w: usize, h: usize, v: f32) -> Self {
        Plane { w, h, d: vec![v; w * h] }
    }
    pub fn row(&self, y: usize) -> &[f32] {
        &self.d[y * self.w..(y + 1) * self.w]
    }
    /// Copy `src` in at (x, y), clipped: `-geometry +x+y -composite`.
    pub fn paste(&mut self, src: &Plane, x: isize, y: isize) {
        let x0 = x.max(0);
        let x1 = (x + src.w as isize).min(self.w as isize);
        if x0 >= x1 {
            return;
        }
        for sy in 0..src.h {
            let dy = y + sy as isize;
            if dy < 0 || dy >= self.h as isize {
                continue;
            }
            let d = dy as usize * self.w;
            let s = sy * src.w;
            self.d[d + x0 as usize..d + x1 as usize].copy_from_slice(&src.d[s + (x0 - x) as usize..s + (x1 - x) as usize]);
        }
    }
    /// Map every value.
    pub fn map(&self, f: impl Fn(f32) -> f32 + Sync + Send) -> Plane {
        let mut o = Plane::new(self.w, self.h);
        let w = self.w;
        rows(&mut o.d, w, |y, row| {
            for (d, &s) in row.iter_mut().zip(self.row(y)) {
                *d = f(s);
            }
        });
        o
    }
    /// Two planes of one size, value by value.
    pub fn zip(&self, b: &Plane, f: impl Fn(f32, f32) -> f32 + Sync + Send) -> Plane {
        assert_eq!((self.w, self.h), (b.w, b.h));
        let mut o = Plane::new(self.w, self.h);
        let w = self.w;
        rows(&mut o.d, w, |y, row| {
            for ((d, &s), &t) in row.iter_mut().zip(self.row(y)).zip(b.row(y)) {
                *d = f(s, t);
            }
        });
        o
    }
    /// The plane at 8 bits, as `-depth 8 gray:-` hands it over.
    pub fn bytes(&self) -> Vec<u8> {
        self.d.iter().map(|&v| to8(v)).collect()
    }
    pub fn from_bytes(w: usize, h: usize, b: &[u8]) -> Plane {
        Plane { w, h, d: b.iter().map(|&v| v as f32 / 255.0).collect() }
    }
    /// Rounded to 8 bits in place, as an intermediate file at depth 8 is.
    pub fn q8(&mut self) {
        for v in self.d.iter_mut() {
            *v = to8(*v) as f32 / 255.0;
        }
    }
    pub fn mean(&self) -> f64 {
        self.d.iter().map(|&v| v as f64).sum::<f64>() / self.d.len().max(1) as f64
    }
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Plane {
        let mut o = Plane::new(w, h);
        for yy in 0..h {
            o.d[yy * w..(yy + 1) * w].copy_from_slice(&self.d[(y + yy) * self.w + x..(y + yy) * self.w + x + w]);
        }
        o
    }
}

/// 0..1 to a byte, as ImageMagick scales a quantum to a char.
#[inline]
pub fn to8(v: f32) -> u8 {
    (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// `f(y, row)` over every row, in parallel with the `par` feature.
pub fn rows<T: Send, F: Fn(usize, &mut [T]) + Sync + Send>(d: &mut [T], w: usize, f: F) {
    #[cfg(feature = "par")]
    d.par_chunks_mut(w.max(1)).enumerate().for_each(|(y, r)| f(y, r));
    #[cfg(not(feature = "par"))]
    d.chunks_mut(w.max(1)).enumerate().for_each(|(y, r)| f(y, r));
}

/// `f(i)` for every i, collected in order, in parallel with `par`.
pub fn par_map<T: Send, F: Fn(usize) -> T + Sync + Send>(n: usize, f: F) -> Vec<T> {
    #[cfg(feature = "par")]
    return (0..n).into_par_iter().map(f).collect();
    #[cfg(not(feature = "par"))]
    return (0..n).map(f).collect();
}

/// ImageMagick's size for `N%` of a side.
pub fn pct(n: usize, p: f64) -> usize {
    ((n as f64 * p / 100.0) + 0.5).floor().max(1.0) as usize
}

/// Python's round(): halves to even, as the script computes its numbers.
pub fn py_round(x: f64) -> i64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
        (r - x.signum()) as i64
    } else {
        r as i64
    }
}

pub const EPS: f64 = 1.0e-12; // MagickEpsilon

/// `-sample`: point sampling at ImageMagick's offsets.
pub fn sample(p: &Plane, w2: usize, h2: usize) -> Plane {
    let off = 0.5 - EPS;
    let xs: Vec<usize> = (0..w2)
        .map(|x| ((((x as f64 + off) * p.w as f64) / w2 as f64) as usize).min(p.w - 1))
        .collect();
    let mut out = Plane::new(w2, h2);
    rows(&mut out.d, w2, |y, row| {
        let sy = (((y as f64 + off) * p.h as f64) / h2 as f64) as usize;
        let src = p.row(sy.min(p.h - 1));
        for (o, &sx) in row.iter_mut().zip(&xs) {
            *o = src[sx];
        }
    });
    out
}

/// `-sample` of a 0/1 mask, to floats.
pub fn sample_u8(m: &[u8], w: usize, h: usize, w2: usize, h2: usize) -> Plane {
    let off = 0.5 - EPS;
    let xs: Vec<usize> = (0..w2).map(|x| ((((x as f64 + off) * w as f64) / w2 as f64) as usize).min(w - 1)).collect();
    let mut out = Plane::new(w2, h2);
    rows(&mut out.d, w2, |y, row| {
        let sy = ((((y as f64 + off) * h as f64) / h2 as f64) as usize).min(h - 1);
        for (o, &sx) in row.iter_mut().zip(&xs) {
            *o = m[sy * w + sx] as f32;
        }
    });
    out
}

// ---------------------------------------------------------------- morphology

/// A flat, symmetric kernel as its rows: (dy, half width).
pub type Kernel = Vec<(isize, usize)>;

/// Octagon:r -- |u| + |v| <= r + r/2 inside a (2r+1) square.
pub fn octagon(r: usize) -> Kernel {
    let lim = r + r / 2;
    (-(r as isize)..=r as isize)
        .map(|dy| (dy, r.min(lim - dy.unsigned_abs())))
        .collect()
}

/// Disk:r -- u^2 + v^2 <= r^2.
pub fn disk(r: usize) -> Kernel {
    let r2 = (r * r) as isize;
    (-(r as isize)..=r as isize)
        .map(|dy| {
            let mut hw = 0;
            while ((hw + 1) * (hw + 1)) as isize + dy * dy <= r2 {
                hw += 1;
            }
            (dy, hw)
        })
        .collect()
}

/// Diamond:r -- |u| + |v| <= r.
pub fn diamond(r: usize) -> Kernel {
    (-(r as isize)..=r as isize).map(|dy| (dy, r - dy.unsigned_abs())).collect()
}

/// Square:r -- the (2r+1) square.
pub fn square(r: usize) -> Kernel {
    (-(r as isize)..=r as isize).map(|dy| (dy, r)).collect()
}

/// Dilate (max) or erode (min) with a flat kernel, edge pixels replicated.
pub fn morph_k<T>(d: &[T], w: usize, h: usize, k: &Kernel, dilate: bool) -> Vec<T>
where
    T: Copy + PartialOrd + Send + Sync + Default,
{
    let pick = |a: T, b: T| if (b > a) == dilate { b } else { a };
    // each output row: the horizontal extreme of every kernel row, combined;
    // worked out per row, so nothing but the output is image-sized
    let mut out = vec![T::default(); w * h];
    rows(&mut out, w, |y, row| {
        let mut tmp = vec![T::default(); w];
        for (i, &(dy, hw)) in k.iter().enumerate() {
            let sy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
            let src = &d[sy * w..(sy + 1) * w];
            let dst: &mut [T] = if i == 0 { &mut *row } else { &mut tmp };
            if hw == 0 {
                dst.copy_from_slice(src);
            } else {
                for x in 0..w {
                    let lo = x.saturating_sub(hw);
                    let hi = (x + hw).min(w - 1);
                    let mut m = src[lo];
                    for &v in &src[lo + 1..=hi] {
                        m = pick(m, v);
                    }
                    dst[x] = m;
                }
            }
            if i > 0 {
                for (o, &v) in row.iter_mut().zip(&tmp) {
                    *o = pick(*o, v);
                }
            }
        }
    });
    out
}

/// Dilate or erode with Octagon:r.
pub fn morph<T>(d: &[T], w: usize, h: usize, r: usize, dilate: bool) -> Vec<T>
where
    T: Copy + PartialOrd + Send + Sync + Default,
{
    morph_k(d, w, h, &octagon(r), dilate)
}

/// The same on a plane, `iter` times.
pub fn morph_p(p: &Plane, k: &Kernel, dilate: bool, iter: usize) -> Plane {
    let mut d = p.d.clone();
    for _ in 0..iter {
        d = morph_k(&d, p.w, p.h, k, dilate);
    }
    Plane { w: p.w, h: p.h, d }
}

// ------------------------------------------------------------------- resize

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Filter {
    Point,
    Box,
    Triangle,
    Lanczos,
    Mitchell,
    Robidoux,
}

impl Filter {
    pub fn support(self) -> f64 {
        match self {
            Filter::Point => 0.0,
            Filter::Box => 0.5,
            Filter::Triangle => 1.0,
            Filter::Lanczos => 3.0,
            Filter::Mitchell | Filter::Robidoux => 2.0,
        }
    }
    pub fn weight(self, x: f64) -> f64 {
        let x = x.abs();
        match self {
            Filter::Point | Filter::Box => 1.0,
            Filter::Triangle => (1.0 - x).max(0.0),
            Filter::Lanczos => {
                if x >= 3.0 {
                    0.0
                } else {
                    sinc(x) * sinc(x / 3.0)
                }
            }
            Filter::Mitchell => cubic_bc(x, 1.0 / 3.0, 1.0 / 3.0),
            Filter::Robidoux => cubic_bc(x, 0.37821575509399867, 0.31089212245300067),
        }
    }
}

fn cubic_bc(x: f64, b: f64, c: f64) -> f64 {
    let c0 = 1.0 - b / 3.0;
    let c1 = -3.0 + 2.0 * b + c;
    let c2 = 2.0 - 1.5 * b - c;
    let c3 = 4.0 / 3.0 * b + 4.0 * c;
    let c4 = -8.0 * c - 2.0 * b;
    let c5 = b + 5.0 * c;
    let c6 = -b / 6.0 - c;
    if x < 1.0 {
        c0 + x * x * (c1 + x * c2)
    } else if x < 2.0 {
        c3 + x * (c4 + x * (c5 + x * c6))
    } else {
        0.0
    }
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        let a = std::f64::consts::PI * x;
        a.sin() / a
    }
}

struct Contrib {
    start: usize,
    w: Vec<f32>,
}

/// ImageMagick's resize weights for one axis (resize.c, HorizontalFilter).
fn contribs(n_in: usize, n_out: usize, f: Filter) -> Vec<Contrib> {
    let factor = n_out as f64 / n_in as f64;
    let mut scale = (1.0 / factor + EPS).max(1.0);
    let mut support = scale * f.support();
    let point = support < 0.5;
    if point {
        support = 0.5;
        scale = 1.0;
    }
    let inv = 1.0 / scale;
    (0..n_out)
        .map(|x| {
            let bisect = (x as f64 + 0.5) / factor + EPS;
            let start = (bisect - support + 0.5).max(0.0) as usize;
            let stop = ((bisect + support + 0.5) as usize).min(n_in).max(start + 1);
            let start = start.min(n_in - 1);
            let mut w: Vec<f64> = (start..stop)
                .map(|j| if point { 1.0 } else { f.weight(inv * (j as f64 - bisect + 0.5)) })
                .collect();
            let s: f64 = w.iter().sum();
            if s != 0.0 && s != 1.0 {
                w.iter_mut().for_each(|v| *v /= s);
            }
            Contrib { start, w: w.into_iter().map(|v| v as f32).collect() }
        })
        .collect()
}

fn resize_h(p: &Plane, w2: usize, f: Filter) -> Plane {
    let cs = contribs(p.w, w2, f);
    let mut out = Plane::new(w2, p.h);
    rows(&mut out.d, w2, |y, row| {
        let src = p.row(y);
        for (o, c) in row.iter_mut().zip(&cs) {
            let mut s = 0.0f32;
            for (k, &wt) in c.w.iter().enumerate() {
                s += wt * src[c.start + k];
            }
            *o = s.clamp(0.0, 1.0);
        }
    });
    out
}

fn resize_v(p: &Plane, h2: usize, f: Filter) -> Plane {
    let cs = contribs(p.h, h2, f);
    let w = p.w;
    let mut out = Plane::new(w, h2);
    rows(&mut out.d, w, |y, row| {
        let c = &cs[y];
        row.fill(0.0);
        for (k, &wt) in c.w.iter().enumerate() {
            for (o, &v) in row.iter_mut().zip(p.row(c.start + k)) {
                *o += wt * v;
            }
        }
        for o in row.iter_mut() {
            *o = o.clamp(0.0, 1.0);
        }
    });
    out
}

/// `-resize WxH!` of an image handed over a row at a time -- a photo kept
/// at 8 bits, or its grey -- so no full-size float copy of it is made.
pub fn resize_rows(w: usize, h: usize, row: &(dyn Fn(usize, &mut [f32]) + Sync), w2: usize, h2: usize, f: Filter) -> Plane {
    let xf = w2 as f64 / w as f64;
    let yf = h2 as f64 / h as f64;
    if xf > yf {
        // along the rows first: w2 x h
        let cs = contribs(w, w2, f);
        let mut mid = Plane::new(w2, h);
        rows(&mut mid.d, w2, |y, out| {
            let mut src = vec![0f32; w];
            row(y, &mut src);
            for (o, c) in out.iter_mut().zip(&cs) {
                let mut s = 0.0f32;
                for (k, &wt) in c.w.iter().enumerate() {
                    s += wt * src[c.start + k];
                }
                *o = s.clamp(0.0, 1.0);
            }
        });
        resize_v(&mid, h2, f)
    } else {
        // down the columns first: w x h2
        let cs = contribs(h, h2, f);
        let mut mid = Plane::new(w, h2);
        rows(&mut mid.d, w, |y, out| {
            let c = &cs[y];
            out.fill(0.0);
            let mut src = vec![0f32; w];
            for (k, &wt) in c.w.iter().enumerate() {
                row(c.start + k, &mut src);
                for (o, &v) in out.iter_mut().zip(&src) {
                    *o += wt * v;
                }
            }
            for o in out.iter_mut() {
                *o = o.clamp(0.0, 1.0);
            }
        });
        resize_h(&mid, w2, f)
    }
}

/// `-resize WxH!` with `-filter`, in ImageMagick's axis order.
pub fn resize(p: &Plane, w2: usize, h2: usize, f: Filter) -> Plane {
    let xf = w2 as f64 / p.w as f64;
    let yf = h2 as f64 / p.h as f64;
    if xf > yf {
        resize_v(&resize_h(p, w2, f), h2, f)
    } else {
        resize_h(&resize_v(p, h2, f), w2, f)
    }
}

/// The filter ImageMagick picks when none is set: none at all for the same
/// size, Mitchell to enlarge, Lanczos to reduce.
pub fn default_filter(w: usize, h: usize, w2: usize, h2: usize) -> Option<Filter> {
    if w == w2 && h == h2 {
        return None;
    }
    let (xf, yf) = (w2 as f64 / w as f64, h2 as f64 / h as f64);
    Some(if xf * yf > 1.0 { Filter::Mitchell } else { Filter::Lanczos })
}

/// `-resize WxH!` without `-filter`.
pub fn resize_auto(p: &Plane, w2: usize, h2: usize) -> Plane {
    match default_filter(p.w, p.h, w2, h2) {
        None => p.clone(),
        Some(f) => resize(p, w2, h2, f),
    }
}

// --------------------------------------------------------------------- blur

/// ImageMagick's Gaussian width for `-blur 0xS` (gem.c).
pub fn gauss_width(sigma: f64) -> usize {
    let alpha = 1.0 / (2.0 * sigma * sigma);
    let beta = 1.0 / ((2.0 * std::f64::consts::PI).sqrt() * sigma);
    let mut width = 5usize;
    loop {
        let j = (width as isize - 1) / 2;
        let norm: f64 = (-j..=j).map(|i| (-((i * i) as f64) * alpha).exp() * beta).sum();
        let v = (-((j * j) as f64) * alpha).exp() * beta / norm;
        if v < 1.0 / 65535.0 || v < EPS {
            break;
        }
        width += 2;
    }
    width - 2
}

/// ImageMagick's "Blur" kernel: each tap the mean of three sub-samples.
pub fn gauss_kernel(sigma: f64) -> Vec<f32> {
    const RANK: isize = 3;
    let width = gauss_width(sigma);
    let v = (width as isize * RANK - 1) / 2;
    let s = sigma * RANK as f64;
    let alpha = 1.0 / (2.0 * s * s);
    let beta = 1.0 / ((2.0 * std::f64::consts::PI).sqrt() * s);
    let mut k = vec![0.0f64; width];
    for u in -v..=v {
        k[((u + v) / RANK) as usize] += (-((u * u) as f64) * alpha).exp() * beta;
    }
    let sum: f64 = k.iter().sum();
    k.into_iter().map(|x| (x / sum) as f32).collect()
}

/// Convolve with a symmetric 1-D kernel along rows, then columns.
fn convolve_sep(p: &Plane, k: &[f32]) -> Plane {
    let r = (k.len() / 2) as isize;
    let (w, h) = (p.w, p.h);
    let mut t = Plane::new(w, h);
    rows(&mut t.d, w, |y, row| {
        let src = p.row(y);
        for x in 0..w {
            let mut s = 0.0f32;
            let x0 = x as isize - r;
            if x0 >= 0 && x0 + (k.len() as isize) <= w as isize {
                for (kv, &v) in k.iter().zip(&src[x0 as usize..x0 as usize + k.len()]) {
                    s += kv * v;
                }
            } else {
                for (i, &kv) in k.iter().enumerate() {
                    let sx = (x0 + i as isize).clamp(0, w as isize - 1) as usize;
                    s += kv * src[sx];
                }
            }
            row[x] = s;
        }
    });
    let mut out = Plane::new(w, h);
    rows(&mut out.d, w, |y, row| {
        row.fill(0.0);
        for (i, &kv) in k.iter().enumerate() {
            let sy = (y as isize + i as isize - r).clamp(0, h as isize - 1) as usize;
            for (o, &v) in row.iter_mut().zip(t.row(sy)) {
                *o += kv * v;
            }
        }
    });
    out
}

/// `-blur 0xS`: horizontal then vertical, edge pixels replicated.
pub fn blur(p: &Plane, sigma: f64) -> Plane {
    if sigma > 24.0 && p.w.min(p.h) > 64 {
        return blur_wide(p, sigma);
    }
    convolve_sep(p, &gauss_kernel(sigma))
}

/// A very wide `-blur` (a colour copy's light estimate: a fifth of the
/// page, 1800 taps) without 1800 taps a pixel. The kernel hardly changes
/// across a block of `f` pixels, so each block counts as its mean, weighed
/// by the kernel's exact sum over it; what lies beyond the edge is the edge
/// pixel itself, replicated, as ImageMagick does -- with a kernel wider
/// than the image that is most of the weight, and averaging it into a
/// block shifted a card's light by nine levels. The result, at the block
/// centres, is interpolated back up.
fn blur_wide(p: &Plane, sigma: f64) -> Plane {
    let k = gauss_kernel(sigma);
    let r = (k.len() / 2) as isize;
    let mut pre = vec![0f64; k.len() + 1];
    for (i, &v) in k.iter().enumerate() {
        pre[i + 1] = pre[i] + v as f64;
    }
    // the kernel's sum over taps [a, b), clipped to it
    let ksum = |a: isize, b: isize| -> f64 {
        let (a, b) = (a.clamp(0, k.len() as isize), b.clamp(0, k.len() as isize));
        if b > a {
            pre[b as usize] - pre[a as usize]
        } else {
            0.0
        }
    };
    let f = ((sigma / 8.0) as usize).clamp(2, 64);
    // one axis: n values -> the blur at each block centre
    let pass = |v: &[f64], n: usize| -> Vec<f64> {
        let nb = n.div_ceil(f);
        let means: Vec<(isize, isize, f64)> = (0..nb)
            .map(|b| {
                let (lo, hi) = (b * f, ((b + 1) * f).min(n));
                (lo as isize, hi as isize, v[lo..hi].iter().sum::<f64>() / (hi - lo) as f64)
            })
            .collect();
        means
            .iter()
            .map(|&(lo, hi, _)| {
                let x = (lo + hi - 1) / 2;
                // tap t sits at pixel x + t - r
                let mut s = v[0] * ksum(0, r - x) + v[n - 1] * ksum(n as isize - x + r, k.len() as isize);
                for &(a, b, m) in &means {
                    s += m * ksum(a - x + r, b - x + r);
                }
                s
            })
            .collect()
    };
    let (w, h) = (p.w, p.h);
    let (cw, ch) = (w.div_ceil(f), h.div_ceil(f));
    // along the rows, every row
    let hs: Vec<Vec<f64>> = par_map(h, |y| pass(&p.row(y).iter().map(|&v| v as f64).collect::<Vec<_>>(), w));
    // down the columns of that
    let vs: Vec<Vec<f64>> = par_map(cw, |c| pass(&(0..h).map(|y| hs[y][c]).collect::<Vec<_>>(), h));
    // back up: the block centres are the sample points
    let centre = |b: usize, n: usize| ((b * f + ((b + 1) * f).min(n) - 1) / 2) as f64;
    let axis = |n: usize, nb: usize| -> Vec<(usize, usize, f64)> {
        (0..n)
            .map(|x| {
                let xf = x as f64;
                let mut b = 0;
                while b + 1 < nb && centre(b + 1, n) <= xf {
                    b += 1;
                }
                if b + 1 >= nb || xf <= centre(0, n) {
                    let bb = if xf <= centre(0, n) { 0 } else { nb - 1 };
                    return (bb, bb, 0.0);
                }
                let (c0, c1) = (centre(b, n), centre(b + 1, n));
                (b, b + 1, (xf - c0) / (c1 - c0))
            })
            .collect()
    };
    let (ax, ay) = (axis(w, cw), axis(h, ch));
    let mut out = Plane::new(w, h);
    rows(&mut out.d, w, |y, row| {
        let (y0, y1, ty) = ay[y];
        for x in 0..w {
            let (x0, x1, tx) = ax[x];
            let top = vs[x0][y0] * (1.0 - tx) + vs[x1][y0] * tx;
            let bot = vs[x0][y1] * (1.0 - tx) + vs[x1][y1] * tx;
            row[x] = (top * (1.0 - ty) + bot * ty) as f32;
        }
    });
    out
}

/// `-statistic StandardDeviation NxN`, as ImageMagick quantizes it: the
/// deviation of the 16-bit values, truncated to an integer, back in 0..1.
pub fn stddev(p: &Plane, n: usize) -> Plane {
    let r = (n / 2) as isize;
    let (w, h) = (p.w, p.h);
    let area = (n * n) as f32;
    let mut out = Plane::new(w, h);
    rows(&mut out.d, w, |y, row| {
        let mut cs = vec![0.0f32; w];
        let mut cq = vec![0.0f32; w];
        for dy in -r..=r {
            let sy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
            for ((a, b), &v) in cs.iter_mut().zip(cq.iter_mut()).zip(p.row(sy)) {
                let v = (v * 65535.0).round();
                *a += v;
                *b += v * v;
            }
        }
        for x in 0..w {
            let (mut s, mut q) = (0.0f32, 0.0f32);
            for dx in -r..=r {
                let sx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
                s += cs[sx];
                q += cq[sx];
            }
            let m = s / area;
            let var = (q / area - m * m).max(0.0);
            row[x] = var.sqrt().trunc() / 65535.0;
        }
    });
    out
}

/// `-unsharp 0xS+gain+threshold`.
pub fn unsharp(p: &Plane, sigma: f64, gain: f32, threshold: f32) -> Plane {
    let b = blur(p, sigma);
    p.zip(&b, |v, bl| {
        let d = v - bl;
        if (2.0 * d).abs() < threshold {
            v
        } else {
            (v + gain * d).clamp(0.0, 1.0)
        }
    })
}

/// A percentage of the quantum range as ImageMagick parses it for a
/// threshold: of QuantumRange+1 (StringToDoubleInterval), so 40% lies a
/// hair above 102/255 and a pixel of exactly 102 is under it.
pub fn pct_thr(pct_: f64) -> f32 {
    (pct_ / 100.0 * 65536.0 / 65535.0) as f32
}

/// `-unsharp` in place.
pub fn unsharp_in(p: &mut Plane, sigma: f64, gain: f32, threshold: f32) {
    let b = blur(p, sigma);
    let w = p.w;
    rows(&mut p.d, w, |y, row| {
        for (v, &bl) in row.iter_mut().zip(b.row(y)) {
            let d = *v - bl;
            if (2.0 * d).abs() >= threshold {
                *v = (*v + gain * d).clamp(0.0, 1.0);
            }
        }
    });
}

/// `-threshold T%`: 1 above, 0 at or below.
pub fn threshold(p: &Plane, pct_: f64) -> Plane {
    let t = pct_thr(pct_);
    p.map(|v| if v > t { 1.0 } else { 0.0 })
}

// ------------------------------------------------------------------ vision

/// `-canny 0xS+lo%+hi%` (feature.c), with its quirks: the 2x2 gradient, the
/// four orientations, and the hysteresis that uses the first row of its
/// own gradient cache as its stack.
pub fn canny(p: &Plane, sigma: f64, lower: f64, upper: f64) -> Vec<u8> {
    let (w, h) = (p.w, p.h);
    let b = convolve_sep(p, &gauss_kernel(sigma));
    #[derive(Clone, Copy, Default)]
    struct Info {
        mag: f64,
        int: f64,
        ori: u8,
        x: isize,
        y: isize,
    }
    let at = |x: isize, y: isize| -> f64 {
        let xx = x.clamp(0, w as isize - 1) as usize;
        let yy = y.clamp(0, h as isize - 1) as usize;
        b.d[yy * w + xx] as f64 * 65535.0
    };
    let mut cache = vec![Info::default(); w * h];
    for y in 0..h as isize {
        for x in 0..w as isize {
            let (p00, p01, p10, p11) = (at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1));
            let dx = 0.5 * (-p00 + p01 - p10 + p11);
            let dy = 0.5 * (p00 + p01 - p10 - p11);
            let mut c = Info { mag: dx.hypot(dy), ..Default::default() };
            if dx.abs() > EPS {
                let s = dy / dx;
                c.ori = if s < 0.0 {
                    if s < -2.41421356237 {
                        0
                    } else if s < -0.414213562373 {
                        1
                    } else {
                        2
                    }
                } else if s > 2.41421356237 {
                    0
                } else if s > 0.414213562373 {
                    3
                } else {
                    2
                };
            }
            cache[y as usize * w + x as usize] = c;
        }
    }
    let idx = |x: isize, y: isize| {
        (y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize
    };
    let mut max = 0.0f64;
    let mut min = 0.0f64;
    for y in 0..h as isize {
        for x in 0..w as isize {
            let c = cache[idx(x, y)];
            let (a, bb) = match c.ori {
                1 => (cache[idx(x - 1, y - 1)], cache[idx(x + 1, y + 1)]),
                2 => (cache[idx(x - 1, y)], cache[idx(x + 1, y)]),
                3 => (cache[idx(x - 1, y + 1)], cache[idx(x + 1, y - 1)]),
                _ => (cache[idx(x, y - 1)], cache[idx(x, y + 1)]),
            };
            let mut ci = c;
            ci.int = if c.mag < a.mag || c.mag < bb.mag { 0.0 } else { c.mag };
            cache[idx(x, y)] = ci;
            min = min.min(ci.int);
            max = max.max(ci.int);
        }
    }
    let lo = lower * (max - min) + min;
    let hi = upper * (max - min) + min;
    let mut e = vec![0u8; w * h];
    for y in 0..h as isize {
        for x in 0..w as isize {
            if e[idx(x, y)] == 0 && cache[idx(x, y)].int >= hi {
                // TraceEdges, literally
                e[idx(x, y)] = 1;
                let mut edge = cache[0];
                edge.x = x;
                edge.y = y;
                cache[0] = edge;
                let mut i: usize = 1;
                while i != 0 {
                    i -= 1;
                    // the stack lives in row 0: a read past its end is
                    // clamped to its last slot (GetMatrixElement), a write
                    // is not (SetMatrixElement) and runs on into row 1
                    let mut edge = cache[i.min(w - 1)];
                    for v in -1..=1isize {
                        for u in -1..=1isize {
                            if u == 0 && v == 0 {
                                continue;
                            }
                            let (xx, yy) = (edge.x + u, edge.y + v);
                            if xx < 0 || yy < 0 || xx >= w as isize || yy >= h as isize {
                                continue;
                            }
                            let px = cache[idx(xx, yy)];
                            if e[idx(xx, yy)] == 0 && px.int >= lo {
                                e[idx(xx, yy)] = 1;
                                edge.x += u;
                                edge.y += v;
                                if i < w * h {
                                    cache[i] = edge;
                                }
                                i += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    e
}

/// `-hough-lines WxH+T`, as the MVG it writes: (x1, y1, x2, y2, count), the
/// numbers at the six significant digits of `%g`.
pub fn hough_lines(e: &[u8], w: usize, h: usize, nw: usize, nh: usize, thr: usize) -> Vec<(f64, f64, f64, f64, f64)> {
    let hough_h = (2f64.sqrt() * w.max(h) as f64) / 2.0;
    let acc_h = (2.0 * hough_h) as usize;
    let acc_w = 180usize;
    let mut acc = vec![0.0f64; acc_w * acc_h];
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let cs: Vec<(f64, f64)> = (0..180).map(|i| {
        let a = (i as f64).to_radians();
        (a.cos(), a.sin())
    }).collect();
    let mround = |x: f64| if x - x.floor() < x.ceil() - x { x.floor() } else { x.ceil() };
    for y in 0..h {
        for x in 0..w {
            if e[y * w + x] == 0 {
                continue;
            }
            for (i, &(c, s)) in cs.iter().enumerate() {
                let r = (x as f64 - cx) * c + (y as f64 - cy) * s;
                let k = mround(r + hough_h) as isize;
                let k = k.clamp(0, acc_h as isize - 1) as usize;
                acc[k * acc_w + i] += 1.0;
            }
        }
    }
    let get = |x: isize, y: isize| {
        acc[(y.clamp(0, acc_h as isize - 1) as usize) * acc_w + x.clamp(0, acc_w as isize - 1) as usize]
    };
    let g6 = |v: f64| -> f64 { format!("{:.5e}", v).parse::<f64>().unwrap() };
    let mut out = vec![];
    let line_count = if thr != 0 { thr } else { w.max(h) / 4 } as f64;
    for y in 0..acc_h as isize {
        for x in 0..acc_w as isize {
            let count = get(x, y);
            if count < line_count {
                continue;
            }
            let mut maxima = count;
            'outer: for v in -((nh / 2) as isize)..=(nh / 2) as isize {
                for u in -((nw / 2) as isize)..=(nw / 2) as isize {
                    if u != 0 || v != 0 {
                        let c = get(x + u, y + v);
                        if c > maxima {
                            maxima = c;
                            // ImageMagick leaves the row loop too, unless
                            // the break came on its last column
                            if u < (nw / 2) as isize {
                                break 'outer;
                            }
                            break;
                        }
                    }
                }
            }
            if maxima > count {
                continue;
            }
            let t = (x as f64).to_radians();
            let r = y as f64 - acc_h as f64 / 2.0;
            let (x1, y1, x2, y2) = if (45..=135).contains(&x) {
                let x1 = 0.0;
                let y1 = (r - (x1 - w as f64 / 2.0) * t.cos()) / t.sin() + h as f64 / 2.0;
                let x2 = w as f64;
                let y2 = (r - (x2 - w as f64 / 2.0) * t.cos()) / t.sin() + h as f64 / 2.0;
                (x1, y1, x2, y2)
            } else {
                let y1 = 0.0;
                let x1 = (r - (y1 - h as f64 / 2.0) * t.sin()) / t.cos() + w as f64 / 2.0;
                let y2 = h as f64;
                let x2 = (r - (y2 - h as f64 / 2.0) * t.sin()) / t.cos() + w as f64 / 2.0;
                (x1, y1, x2, y2)
            };
            out.push((g6(x1), g6(y1), g6(x2), g6(y2), maxima));
        }
    }
    out
}

/// A component of `-connected-components 8`: its area and bounding box.
#[derive(Clone, Copy, Debug)]
pub struct Blob {
    pub area: usize,
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// The 8-connected components of the set pixels of a mask.
pub fn components8(m: &[u8], w: usize, h: usize) -> Vec<Blob> {
    let mut seen = vec![false; w * h];
    let mut out = vec![];
    let mut stack = vec![];
    for s in 0..w * h {
        if m[s] == 0 || seen[s] {
            continue;
        }
        seen[s] = true;
        stack.push(s);
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (w, h, 0, 0, 0);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            n += 1;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            for dy in -1..=1isize {
                for dx in -1..=1isize {
                    let (xx, yy) = (x as isize + dx, y as isize + dy);
                    if xx < 0 || yy < 0 || xx >= w as isize || yy >= h as isize {
                        continue;
                    }
                    let j = yy as usize * w + xx as usize;
                    if m[j] != 0 && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        out.push(Blob { area: n, x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 });
    }
    out
}

/// `-trim`'s box of a mask (attribute.c, GetImageBoundingBox), or None when
/// the whole image is its background.
pub fn trim_box(m: &Plane) -> Option<(usize, usize, usize, usize)> {
    let (w, h) = (m.w, m.h);
    let px = |x: usize, y: usize| m.d[y * w + x];
    let t = [px(0, 0), px(w - 1, 0), px(0, h - 1), px(w - 1, h - 1)];
    let (mut bx, mut by) = (w as isize, h as isize);
    let (mut bw, mut bh) = ((w == 1) as isize, (h == 1) as isize);
    for y in 0..h {
        let (mut x0, mut y0, mut ww, mut hh) = (bx, by, bw, bh);
        for x in 0..w {
            let p = px(x, y);
            let (xi, yi) = (x as isize, y as isize);
            if xi < x0 && p != t[0] {
                x0 = xi;
            }
            if xi > ww && p != t[1] {
                ww = xi;
            }
            if yi < y0 && p != t[0] {
                y0 = yi;
            }
            if yi > hh && p != t[2] {
                hh = yi;
            }
            if xi < ww && yi > hh && p != t[3] {
                ww = xi;
                hh = yi;
            }
        }
        bx = bx.min(x0);
        by = by.min(y0);
        bw = bw.max(ww);
        bh = bh.max(hh);
    }
    if bw == 0 || bh == 0 || bx >= w as isize || by >= h as isize {
        return None;
    }
    let ww = bw - (bx - 1);
    let hh = bh - (by - 1);
    if ww <= 0 || hh <= 0 {
        return None;
    }
    Some((bx as usize, by as usize, ww as usize, hh as usize))
}

/// The angle `-deskew T%` measures (shear.c, RadonTransform): a pixel is
/// dark when any of its channels is under the threshold.
pub fn deskew_angle(ch: &[&Plane], thr: f32) -> f64 {
    let (w, h) = (ch[0].w, ch[0].h);
    let mut width = 1usize;
    while width < (w + 7) / 8 {
        width <<= 1;
    }
    let bits: Vec<u16> = (0..256u32).map(|j| j.count_ones() as u16).collect();
    let dark = |x: usize, y: usize| ch.iter().any(|p| p.d[y * w + x] < thr);
    let mut proj = vec![0u64; 2 * width - 1];
    for pass in 0..2 {
        let mut src = vec![0u16; width * h];
        for y in 0..h {
            let (mut bit, mut byte) = (0u32, 0u32);
            let mut i: isize = if pass == 0 { ((w + 7) / 8) as isize } else { 0 };
            let mut put = |v: u16, i: &mut isize| {
                if pass == 0 {
                    *i -= 1;
                    src[y * width + *i as usize] = v;
                } else {
                    src[y * width + *i as usize] = v;
                    *i += 1;
                }
            };
            for x in 0..w {
                byte <<= 1;
                if dark(x, y) {
                    byte |= 1;
                }
                bit += 1;
                if bit == 8 {
                    put(bits[byte as usize], &mut i);
                    bit = 0;
                    byte = 0;
                }
            }
            if bit != 0 {
                byte <<= 8 - bit;
                put(bits[(byte & 0xff) as usize], &mut i);
            }
        }
        radon_projection(&mut src, width, h, if pass == 0 { -1 } else { 1 }, &mut proj);
    }
    let (mut maxp, mut skew) = (0u64, 0isize);
    for (i, &p) in proj.iter().enumerate() {
        if p > maxp {
            skew = i as isize - width as isize + 1;
            maxp = p;
        }
    }
    -(skew as f64 / width as f64 / 8.0).atan().to_degrees()
}

fn radon_projection(src: &mut Vec<u16>, cols: usize, rows_: usize, sign: isize, proj: &mut [u64]) {
    let mut p = std::mem::take(src);
    let mut q = vec![0u16; cols * rows_];
    let g = |m: &Vec<u16>, x: usize, y: usize| m[y.min(rows_ - 1) * cols + x.min(cols - 1)];
    let mut step = 1;
    while step < cols {
        let mut x = 0;
        while x < cols {
            for i in 0..step {
                let mut y = 0;
                while (y as isize) < rows_ as isize - i as isize - 1 {
                    let e = g(&p, x + i, y);
                    let n = g(&p, x + i + step, y + i).wrapping_add(e);
                    q[y * cols + x + 2 * i] = n;
                    let n = g(&p, x + i + step, y + i + 1).wrapping_add(e);
                    q[y * cols + x + 2 * i + 1] = n;
                    y += 1;
                }
                while (y as isize) < rows_ as isize - i as isize {
                    let e = g(&p, x + i, y);
                    let n = g(&p, x + i + step, y + i).wrapping_add(e);
                    q[y * cols + x + 2 * i] = n;
                    q[y * cols + x + 2 * i + 1] = e;
                    y += 1;
                }
                while y < rows_ {
                    let e = g(&p, x + i, y);
                    q[y * cols + x + 2 * i] = e;
                    q[y * cols + x + 2 * i + 1] = e;
                    y += 1;
                }
            }
            x += 2 * step;
        }
        std::mem::swap(&mut p, &mut q);
        step *= 2;
    }
    for x in 0..cols {
        let mut sum = 0u64;
        for y in 0..rows_ - 1 {
            let d = p[y * cols + x] as i64 - p[(y + 1) * cols + x] as i64;
            sum = sum.wrapping_add((d * d) as u64);
        }
        proj[(cols as isize + sign * x as isize - 1) as usize] = sum;
    }
}

// What ImageMagick 7.1.2 itself says, checked with `-define
// morphology:showKernel=1` and on tiny images; the port leans on each.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernels() {
        // Octagon:1 is a plus, :2 a 5x5 without corners, :3 a 7x7 octagon
        assert_eq!(octagon(1), vec![(-1, 0), (0, 1), (1, 0)]);
        assert_eq!(octagon(2), vec![(-2, 1), (-1, 2), (0, 2), (1, 2), (2, 1)]);
        assert_eq!(octagon(3), vec![(-3, 1), (-2, 2), (-1, 3), (0, 3), (1, 3), (2, 2), (3, 1)]);
        assert_eq!(disk(2), vec![(-2, 0), (-1, 1), (0, 2), (1, 1), (2, 0)]);
        assert_eq!(disk(3), vec![(-3, 0), (-2, 2), (-1, 2), (0, 3), (1, 2), (2, 2), (3, 0)]);
        assert_eq!(diamond(2), vec![(-2, 0), (-1, 1), (0, 2), (1, 1), (2, 0)]);
    }

    #[test]
    fn blur_widths() {
        assert_eq!(gauss_width(6.0), 49);
        assert_eq!(gauss_width(1.3333), 11);
        let k = gauss_kernel(6.0);
        assert!((k.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!((k[24] - 0.066425).abs() < 1e-5, "{}", k[24]);
    }

    #[test]
    fn sample_picks() {
        // magick -size 6x1 gradient: -sample 50% keeps columns 0, 2, 4
        let p = Plane { w: 6, h: 1, d: (0..6).map(|v| v as f32).collect() };
        assert_eq!(sample(&p, 3, 1).d, vec![0.0, 2.0, 4.0]);
        assert_eq!(pct(1534, 33.3333), 511);
        assert_eq!(pct(767, 12.0), 92);
    }

    #[test]
    fn python_rounding() {
        assert_eq!(py_round(2.5), 2);
        assert_eq!(py_round(3.5), 4);
        assert_eq!(py_round(2.0), 2);
        assert_eq!(py_round(1.6667), 2);
    }
}
