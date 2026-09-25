//! The ImageMagick operators a4norm's page finishing uses, reproduced with
//! ImageMagick's own geometry: where a sample lands, how a resize weighs its
//! neighbours, how wide a Gaussian is, which pixels an Octagon kernel covers.
//! Values are f32 in 0..1; what ImageMagick clamps between operators (Q16
//! without HDRI, the browser build) is clamped here too.

#[cfg(feature = "par")]
use rayon::prelude::*;

/// One channel.
#[derive(Clone)]
pub struct Plane {
    pub w: usize,
    pub h: usize,
    pub d: Vec<f32>,
}

impl Plane {
    pub fn new(w: usize, h: usize) -> Self {
        Plane {
            w,
            h,
            d: vec![0.0; w * h],
        }
    }
    pub fn row(&self, y: usize) -> &[f32] {
        &self.d[y * self.w..(y + 1) * self.w]
    }
}

/// `f(y, row)` over every row, in parallel with the `par` feature.
pub fn rows<T: Send, F: Fn(usize, &mut [T]) + Sync + Send>(d: &mut [T], w: usize, f: F) {
    #[cfg(feature = "par")]
    d.par_chunks_mut(w).enumerate().for_each(|(y, r)| f(y, r));
    #[cfg(not(feature = "par"))]
    d.chunks_mut(w).enumerate().for_each(|(y, r)| f(y, r));
}

/// ImageMagick's size for `N%` of a side.
pub fn pct(n: usize, p: f64) -> usize {
    ((n as f64 * p / 100.0) + 0.5).floor().max(1.0) as usize
}

/// Python's round(): halves to even, as the script computes its kernels.
pub fn py_round(x: f64) -> i64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
        (r - x.signum()) as i64
    } else {
        r as i64
    }
}

const EPS: f64 = 1.0e-12; // MagickEpsilon

/// `-sample`: point sampling at ImageMagick's offsets.
pub fn sample(p: &Plane, w2: usize, h2: usize) -> Plane {
    let off = 0.5 - EPS;
    let xs: Vec<usize> = (0..w2)
        .map(|x| (((x as f64 + off) * p.w as f64) / w2 as f64) as usize)
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

/// The rows of an Octagon:r kernel: (dy, half width). ImageMagick keeps
/// |u| + |v| <= r + r/2 inside a (2r+1) square.
fn octagon(r: usize) -> Vec<(isize, usize)> {
    let lim = r + r / 2;
    (-(r as isize)..=r as isize)
        .map(|dy| (dy, r.min(lim - dy.unsigned_abs())))
        .collect()
}

/// Dilate (max) or erode (min) with Octagon:r, edge pixels replicated.
pub fn morph<T>(d: &[T], w: usize, h: usize, r: usize, dilate: bool) -> Vec<T>
where
    T: Copy + PartialOrd + Send + Sync + Default,
{
    let pick = |a: T, b: T| if (b > a) == dilate { b } else { a };
    let spans = octagon(r);
    let mut halves: Vec<usize> = spans.iter().map(|s| s.1).collect();
    halves.sort();
    halves.dedup();
    // the horizontal extreme over each half width the kernel needs
    let horiz: Vec<(usize, Vec<T>)> = halves
        .iter()
        .map(|&k| {
            let mut o = vec![T::default(); w * h];
            rows(&mut o, w, |y, row| {
                let src = &d[y * w..(y + 1) * w];
                for x in 0..w {
                    let lo = x.saturating_sub(k);
                    let hi = (x + k).min(w - 1);
                    let mut m = src[lo];
                    for &v in &src[lo + 1..=hi] {
                        m = pick(m, v);
                    }
                    row[x] = m;
                }
            });
            (k, o)
        })
        .collect();
    let mut out = vec![T::default(); w * h];
    rows(&mut out, w, |y, row| {
        let mut first = true;
        for &(dy, k) in &spans {
            let sy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
            let src = &horiz.iter().find(|p| p.0 == k).unwrap().1[sy * w..(sy + 1) * w];
            if first {
                row.copy_from_slice(src);
                first = false;
            } else {
                for (o, &v) in row.iter_mut().zip(src) {
                    *o = pick(*o, v);
                }
            }
        }
    });
    out
}

#[derive(Clone, Copy)]
pub enum Filter {
    Lanczos,
    Triangle,
}

impl Filter {
    fn support(self) -> f64 {
        match self {
            Filter::Lanczos => 3.0,
            Filter::Triangle => 1.0,
        }
    }
    fn weight(self, x: f64) -> f64 {
        let x = x.abs();
        match self {
            Filter::Lanczos => {
                if x >= 3.0 {
                    0.0
                } else {
                    sinc(x) * sinc(x / 3.0)
                }
            }
            Filter::Triangle => (1.0 - x).max(0.0),
        }
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
            let stop = ((bisect + support + 0.5) as usize).min(n_in);
            let mut w: Vec<f64> = (start..stop)
                .map(|j| {
                    if point {
                        1.0
                    } else {
                        f.weight(inv * (j as f64 - bisect + 0.5))
                    }
                })
                .collect();
            let s: f64 = w.iter().sum();
            if s != 0.0 && s != 1.0 {
                w.iter_mut().for_each(|v| *v /= s);
            }
            Contrib {
                start,
                w: w.into_iter().map(|v| v as f32).collect(),
            }
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

/// ImageMagick's Gaussian width for `-blur 0xS` (gem.c).
fn gauss_width(sigma: f64) -> usize {
    let alpha = 1.0 / (2.0 * sigma * sigma);
    let beta = 1.0 / ((2.0 * std::f64::consts::PI).sqrt() * sigma);
    let mut width = 5usize;
    loop {
        let j = (width as isize - 1) / 2;
        let norm: f64 = (-j..=j)
            .map(|i| (-((i * i) as f64) * alpha).exp() * beta)
            .sum();
        let v = (-((j * j) as f64) * alpha).exp() * beta / norm;
        if v < 1.0 / 65535.0 || v < EPS {
            break;
        }
        width += 2;
    }
    width - 2
}

/// ImageMagick's "Blur" kernel: each tap the mean of three sub-samples.
fn gauss_kernel(sigma: f64) -> Vec<f32> {
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

/// `-blur 0xS`: horizontal then vertical, edge pixels replicated.
pub fn blur(p: &Plane, sigma: f64) -> Plane {
    let k = gauss_kernel(sigma);
    let r = (k.len() / 2) as isize;
    let (w, h) = (p.w, p.h);
    let mut t = Plane::new(w, h);
    rows(&mut t.d, w, |y, row| {
        let src = p.row(y);
        for x in 0..w {
            let mut s = 0.0f32;
            for (i, &kv) in k.iter().enumerate() {
                let sx = (x as isize + i as isize - r).clamp(0, w as isize - 1) as usize;
                s += kv * src[sx];
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

// What ImageMagick 7.1.2 itself says, checked with `-define
// morphology:showKernel=1` and on tiny images; the port leans on each.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octagon_rows() {
        // Octagon:1 is a plus, :2 a 5x5 without corners, :3 a 7x7 octagon
        assert_eq!(octagon(1), vec![(-1, 0), (0, 1), (1, 0)]);
        assert_eq!(octagon(2), vec![(-2, 1), (-1, 2), (0, 2), (1, 2), (2, 1)]);
        assert_eq!(
            octagon(3),
            vec![(-3, 1), (-2, 2), (-1, 3), (0, 3), (1, 3), (2, 2), (3, 1)]
        );
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
        let p = Plane {
            w: 6,
            h: 1,
            d: (0..6).map(|v| v as f32).collect(),
        };
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
