//! A page in memory -- one plane for grey, three for colour -- and the
//! ImageMagick operators that work on whole images: the warps, the
//! composites, the drawing a4norm does.

use crate::ops::{self, rows, Filter, Plane, EPS};

#[derive(Clone, Debug)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub c: Vec<Plane>,
}

pub const LUMA: [f32; 3] = [0.212656, 0.715158, 0.072186];

impl Img {
    pub fn from_planes(c: Vec<Plane>) -> Img {
        Img { w: c[0].w, h: c[0].h, c }
    }
    pub fn solid(w: usize, h: usize, rgb: [f32; 3]) -> Img {
        Img::from_planes(rgb.iter().map(|&v| Plane::filled(w, h, v)).collect())
    }
    pub fn from_rgb8(px: &[u8], w: usize, h: usize) -> Img {
        let mut c = vec![Plane::new(w, h), Plane::new(w, h), Plane::new(w, h)];
        for (k, p) in c.iter_mut().enumerate() {
            rows(&mut p.d, w, |y, row| {
                let src = &px[y * w * 3..(y + 1) * w * 3];
                for (x, o) in row.iter_mut().enumerate() {
                    *o = src[x * 3 + k] as f32 / 255.0;
                }
            });
        }
        Img { w, h, c }
    }
    pub fn from_gray8(px: &[u8], w: usize, h: usize) -> Img {
        Img::from_planes(vec![Plane::from_bytes(w, h, px)])
    }
    pub fn is_gray(&self) -> bool {
        self.c.len() == 1
    }
    /// Three planes, whatever it was.
    pub fn rgb(&self) -> Img {
        if self.c.len() == 3 {
            self.clone()
        } else {
            Img::from_planes(vec![self.c[0].clone(), self.c[0].clone(), self.c[0].clone()])
        }
    }
    /// Interleaved RGB bytes.
    pub fn to_rgb8(&self) -> Vec<u8> {
        let w = self.w;
        let mut px = vec![0u8; w * self.h * 3];
        let c = &self.c;
        let n = c.len();
        rows(&mut px, w * 3, |y, row| {
            for x in 0..w {
                for k in 0..3 {
                    row[x * 3 + k] = ops::to8(c[k.min(n - 1)].d[y * w + x]);
                }
            }
        });
        px
    }
    /// `-colorspace gray`: Rec. 709 luma of the stored values.
    pub fn gray(&self) -> Plane {
        if self.c.len() == 1 {
            return self.c[0].clone();
        }
        let mut g = Plane::new(self.w, self.h);
        let [r, gg, b] = [&self.c[0], &self.c[1], &self.c[2]];
        let w = self.w;
        rows(&mut g.d, w, |y, row| {
            let (r, gg, b) = (r.row(y), gg.row(y), b.row(y));
            for x in 0..w {
                row[x] = LUMA[0] * r[x] + LUMA[1] * gg[x] + LUMA[2] * b[x];
            }
        });
        g
    }
    /// Every plane through `f`.
    pub fn each(&self, f: impl Fn(&Plane) -> Plane + Sync + Send) -> Img {
        Img::from_planes(ops::par_map(self.c.len(), |k| f(&self.c[k])))
    }
    /// Round to 8 bits, as each of the script's intermediate files is.
    pub fn q8(&mut self) {
        for p in self.c.iter_mut() {
            p.q8();
        }
    }
    pub fn resize(&self, w2: usize, h2: usize, f: Filter) -> Img {
        self.each(|p| ops::resize(p, w2, h2, f))
    }
    /// `-resize WxH!` with ImageMagick's default filter.
    pub fn resize_auto(&self, w2: usize, h2: usize) -> Img {
        self.each(|p| ops::resize_auto(p, w2, h2))
    }
    /// `-resize P%`.
    pub fn resize_pct(&self, pct: f64) -> Img {
        self.resize_auto(ops::pct(self.w, pct), ops::pct(self.h, pct))
    }
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Img {
        let w = w.min(self.w - x);
        let h = h.min(self.h - y);
        self.each(|p| p.crop(x, y, w, h))
    }
    /// `-rotate 90|180|270`, clockwise.
    pub fn rotate(&self, deg: i32) -> Img {
        let deg = deg.rem_euclid(360);
        if deg == 0 {
            return self.clone();
        }
        self.each(|p| {
            let (w, h) = (p.w, p.h);
            match deg {
                180 => Plane { w, h, d: p.d.iter().rev().copied().collect() },
                90 => {
                    let mut o = Plane::new(h, w);
                    for y in 0..w {
                        for x in 0..h {
                            o.d[y * h + x] = p.d[(h - 1 - x) * w + y];
                        }
                    }
                    o
                }
                _ => {
                    let mut o = Plane::new(h, w);
                    for y in 0..w {
                        for x in 0..h {
                            o.d[y * h + x] = p.d[x * w + (w - 1 - y)];
                        }
                    }
                    o
                }
            }
        })
    }
    /// `+append` (side by side) or `-append` (stacked); the images must
    /// agree in the other side.
    pub fn append(a: &Img, b: &Img, side_by_side: bool) -> Img {
        let n = a.c.len().max(b.c.len());
        let (a, b) = if n == 3 { (a.rgb(), b.rgb()) } else { (a.clone(), b.clone()) };
        Img::from_planes(
            (0..n)
                .map(|k| {
                    let (p, q) = (&a.c[k], &b.c[k]);
                    if side_by_side {
                        let h = p.h.max(q.h);
                        let w = p.w + q.w;
                        let mut o = Plane::filled(w, h, 1.0);
                        for y in 0..p.h {
                            o.d[y * w..y * w + p.w].copy_from_slice(p.row(y));
                        }
                        for y in 0..q.h {
                            o.d[y * w + p.w..y * w + w].copy_from_slice(q.row(y));
                        }
                        o
                    } else {
                        let w = p.w.max(q.w);
                        let mut o = Plane::filled(w, p.h + q.h, 1.0);
                        for y in 0..p.h {
                            o.d[y * w..y * w + p.w].copy_from_slice(p.row(y));
                        }
                        for y in 0..q.h {
                            o.d[(p.h + y) * w..(p.h + y) * w + q.w].copy_from_slice(q.row(y));
                        }
                        o
                    }
                })
                .collect(),
        )
    }
    /// `%[fx:mean]`: over every channel.
    pub fn mean(&self) -> f64 {
        self.c.iter().map(|p| p.mean()).sum::<f64>() / self.c.len() as f64
    }
    /// Lay `top` over this image at (x, y), blended by `alpha` (top's own
    /// size, 0..1) when given: `-geometry +x+y -compose Over -composite`.
    pub fn over(&mut self, top: &Img, alpha: Option<&Plane>, x: isize, y: isize) {
        let top = if self.c.len() == 3 { top.rgb() } else { top.clone() };
        let (tw, th) = (top.w as isize, top.h as isize);
        let y0 = y.max(0);
        let y1 = (y + th).min(self.h as isize);
        let x0 = x.max(0);
        let x1 = (x + tw).min(self.w as isize);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let w = self.w;
        for (k, p) in self.c.iter_mut().enumerate() {
            let t = &top.c[k];
            rows(&mut p.d[y0 as usize * w..y1 as usize * w], w, |yy, row| {
                let sy = (y0 + yy as isize - y) as usize;
                for xx in x0..x1 {
                    let sx = (xx - x) as usize;
                    let v = t.d[sy * t.w + sx];
                    let o = &mut row[xx as usize];
                    *o = match alpha {
                        None => v,
                        Some(a) => {
                            let a = a.d[sy * a.w + sx];
                            v * a + *o * (1.0 - a)
                        }
                    };
                }
            });
        }
    }
    /// `-compose Screen` of a 0/1 (or grey) mask of the same size.
    pub fn screen(&mut self, m: &Plane) {
        for p in self.c.iter_mut() {
            let w = p.w;
            rows(&mut p.d, w, |y, row| {
                for (o, &a) in row.iter_mut().zip(m.row(y)) {
                    *o = *o + a - *o * a;
                }
            });
        }
    }
    /// `-level lo%,hi%`.
    pub fn level(&self, lo: f64, hi: f64) -> Img {
        let (lo, hi) = ((lo / 100.0) as f32, (hi / 100.0) as f32);
        let span = (hi - lo).max(1e-6);
        self.each(|p| p.map(|v| ((v - lo) / span).clamp(0.0, 1.0)))
    }
    /// `-contrast-stretch B%xW%`: one histogram of the intensity, one map
    /// for every channel.
    pub fn contrast_stretch(&mut self, black_clip: f64, white_clip: f64) {
        let gray = self.gray();
        let mut hist = vec![0u64; 65536];
        for &v in &gray.d {
            hist[((v * 65535.0 + 0.5).max(0.0) as usize).min(65535)] += 1;
        }
        let n = (self.w * self.h) as f64;
        let bp = black_clip * n / 100.0;
        let wp = white_clip * n / 100.0;
        let mut acc = 0.0;
        let mut black = 65535usize;
        for (j, &c) in hist.iter().enumerate() {
            acc += c as f64;
            if acc > bp {
                black = j;
                break;
            }
        }
        acc = 0.0;
        let mut white = 0usize;
        for j in (1..=65535).rev() {
            acc += hist[j] as f64;
            if acc > wp {
                white = j;
                break;
            }
        }
        if white <= black {
            return;
        }
        let (b, span) = (black as f32, (white - black) as f32);
        for p in self.c.iter_mut() {
            let w = p.w;
            rows(&mut p.d, w, |_, row| {
                for v in row.iter_mut() {
                    let q = (*v * 65535.0 + 0.5).floor();
                    *v = ((q - b) / span).clamp(0.0, 1.0);
                }
            });
        }
    }
    /// The HSL saturation of each pixel (colorspace.c, ConvertRGBToHSL).
    pub fn hsl_saturation(&self) -> Plane {
        if self.c.len() == 1 {
            return Plane::new(self.w, self.h);
        }
        let (r, g, b) = (&self.c[0], &self.c[1], &self.c[2]);
        let mut o = Plane::new(self.w, self.h);
        let w = self.w;
        rows(&mut o.d, w, |y, row| {
            let (r, g, b) = (r.row(y), g.row(y), b.row(y));
            for x in 0..w {
                let mx = r[x].max(g[x]).max(b[x]) as f64;
                let mn = r[x].min(g[x]).min(b[x]) as f64;
                let c = mx - mn;
                let l = (mx + mn) / 2.0;
                row[x] = if c <= 0.0 {
                    0.0
                } else if l <= 0.5 {
                    (c * safe_recip(2.0 * l)) as f32
                } else {
                    (c * safe_recip(2.0 - 2.0 * l)) as f32
                };
            }
        });
        o
    }
}

/// Anything pixels can be read from: a page in floats, or a photo kept at
/// 8 bits (a 12 MP photo in floats is 144 MB, in bytes 36 MB).
pub trait Pix: Sync {
    fn dims(&self) -> (usize, usize);
    fn nc(&self) -> usize;
    /// channel k at pixel index i (y * w + x)
    fn at(&self, k: usize, i: usize) -> f32;
    /// row y of channel k, or of the grey when k is GRAY
    fn row_into(&self, k: usize, y: usize, out: &mut [f32]);
}

pub const GRAY: usize = usize::MAX;

impl Pix for Img {
    fn dims(&self) -> (usize, usize) {
        (self.w, self.h)
    }
    fn nc(&self) -> usize {
        self.c.len()
    }
    fn at(&self, k: usize, i: usize) -> f32 {
        self.c[k].d[i]
    }
    fn row_into(&self, k: usize, y: usize, out: &mut [f32]) {
        let w = self.w;
        if k != GRAY {
            out.copy_from_slice(self.c[k].row(y));
        } else if self.c.len() == 1 {
            out.copy_from_slice(self.c[0].row(y));
        } else {
            let (r, g, b) = (self.c[0].row(y), self.c[1].row(y), self.c[2].row(y));
            for x in 0..w {
                out[x] = LUMA[0] * r[x] + LUMA[1] * g[x] + LUMA[2] * b[x];
            }
        }
    }
}

/// A photo as decoded: interleaved RGB bytes.
pub struct Src {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Pix for Src {
    fn dims(&self) -> (usize, usize) {
        (self.w, self.h)
    }
    fn nc(&self) -> usize {
        3
    }
    fn at(&self, k: usize, i: usize) -> f32 {
        self.px[i * 3 + k] as f32 / 255.0
    }
    fn row_into(&self, k: usize, y: usize, out: &mut [f32]) {
        let row = &self.px[y * self.w * 3..(y + 1) * self.w * 3];
        if k != GRAY {
            for (x, o) in out.iter_mut().enumerate() {
                *o = row[x * 3 + k] as f32 / 255.0;
            }
        } else {
            for (x, o) in out.iter_mut().enumerate() {
                let (r, g, b) = (row[x * 3] as f32, row[x * 3 + 1] as f32, row[x * 3 + 2] as f32);
                *o = (LUMA[0] * r + LUMA[1] * g + LUMA[2] * b) / 255.0;
            }
        }
    }
}

impl Src {
    /// The photo in floats, for a page processed at its full size.
    pub fn to_img(&self) -> Img {
        Img::from_rgb8(&self.px, self.w, self.h)
    }
}

/// `-resize WxH!` (ImageMagick's default filter) of any image.
pub fn resize_any(src: &dyn Pix, w2: usize, h2: usize) -> Img {
    let (w, h) = src.dims();
    Img::from_planes(ops::par_map(src.nc(), |k| channel_resized(src, k, w, h, w2, h2)))
}

/// `-colorspace gray -resize WxH!` of any image.
pub fn gray_resized(src: &dyn Pix, w2: usize, h2: usize) -> Plane {
    let (w, h) = src.dims();
    channel_resized(src, GRAY, w, h, w2, h2)
}

fn channel_resized(src: &dyn Pix, k: usize, w: usize, h: usize, w2: usize, h2: usize) -> Plane {
    let row = |y: usize, out: &mut [f32]| src.row_into(k, y, out);
    match ops::default_filter(w, h, w2, h2) {
        Some(f) => ops::resize_rows(w, h, &row, w2, h2, f),
        None => {
            let mut p = Plane::new(w, h);
            rows(&mut p.d, w, |y, out| row(y, out));
            p
        }
    }
}

fn safe_recip(x: f64) -> f64 {
    let s = if x < 0.0 { -1.0 } else { 1.0 };
    if s * x >= EPS {
        1.0 / x
    } else {
        s / EPS
    }
}

// ------------------------------------------------------------ the warps (EWA)

/// ImageMagick's elliptical weighted average over a cylindrical filter
/// (resample.c): a lookup table of 1024 squared radii.
struct Ewa {
    lut: Vec<f64>,
    support: f64,
}

impl Ewa {
    fn new(f: Filter) -> Ewa {
        let support = f.support();
        let rs = support * (1.0 / 1024.0f64).sqrt();
        Ewa { lut: (0..1024).map(|q| f.weight((q as f64).sqrt() * rs)).collect(), support }
    }

    /// One output pixel: the source at (u0, v0) (pixel centres at integers),
    /// with the local derivatives of the mapping. Outside the source is
    /// white, as `-virtual-pixel white` and `-background white` make it.
    #[allow(clippy::too_many_arguments)]
    fn pixel(&self, src: &dyn Pix, u0: f64, v0: f64, dux: f64, duy: f64, dvx: f64, dvy: f64, out: &mut [f32]) {
        let (sw, sh) = src.dims();
        let (cols, rows_) = (sw as f64, sh as f64);
        let nc = src.nc();
        let get = |u: isize, v: isize, k: usize| -> f32 {
            if u < 0 || v < 0 || u >= sw as isize || v >= sh as isize {
                1.0
            } else {
                src.at(k, v as usize * sw + u as usize)
            }
        };
        // ClampUpAxes(dux, dvx, duy, dvy): singular values clamped up to 1
        let (a, b, c, d) = (dux, duy, dvx, dvy);
        let n11 = a * a + b * b;
        let n12 = a * c + b * d;
        let n22 = c * c + d * d;
        let det = a * d - b * c;
        let tdet = det + det;
        let fro = n11 + n22;
        let disc = (fro + tdet) * (fro - tdet);
        let sd = if disc > 0.0 { disc.sqrt() } else { 0.0 };
        let s1 = 0.5 * (fro + sd);
        let s2 = 0.5 * (fro - sd);
        let m11 = s1 - n11;
        let m22 = s1 - n22;
        let (t11, t21) = if m11 * m11 >= m22 * m22 { (n12, m11) } else { (m22, n12) };
        let norm = (t11 * t11 + t21 * t21).sqrt();
        let (u11, u21) = if norm > 0.0 { (t11 / norm, t21 / norm) } else { (1.0, 0.0) };
        let major = if s1 <= 1.0 { 1.0 } else { s1.sqrt() };
        let minor = if s2 <= 1.0 { 1.0 } else { s2.sqrt() };
        let (mx, my) = (u11 * major, u21 * major);
        let (nx, ny) = (-u21 * minor, u11 * minor);
        let mut aa = my * my + ny * ny;
        let mut bb = -2.0 * (mx * my + nx * ny);
        let mut cc = mx * mx + nx * nx;
        let mut f = major * minor;
        f *= f;
        f *= self.support * self.support;
        let den = aa * cc - 0.25 * bb * bb;
        let ulimit = (cc * f / den).sqrt();
        let vlimit = (aa * f / den).sqrt();
        let uwidth = (f / aa).sqrt();
        let slope = -bb / (2.0 * aa);
        let limit = uwidth * vlimit > 4.0 * cols * rows_;
        if limit || u0 + ulimit < 0.0 || u0 - ulimit > cols - 1.0 || v0 + vlimit < 0.0 || v0 - vlimit > rows_ - 1.0 {
            let (u, v) = (u0.floor() as isize, v0.floor() as isize);
            for (k, o) in out.iter_mut().enumerate() {
                *o = get(u, v, k.min(nc - 1));
            }
            return;
        }
        let scale = 1024.0 / f;
        aa *= scale;
        bb *= scale;
        cc *= scale;
        let v1 = (v0 - vlimit).ceil() as isize;
        let v2 = (v0 + vlimit).floor() as isize;
        let mut u1 = u0 + (v1 as f64 - v0) * slope - uwidth;
        let uw = (2.0 * uwidth) as isize + 1;
        let ddq = 2.0 * aa;
        let mut acc = [0f64; 3];
        let mut div = 0f64;
        for v in v1..=v2 {
            let u = u1.ceil() as isize;
            u1 += slope;
            let uu = u as f64 - u0;
            let vv = v as f64 - v0;
            let mut q = (aa * uu + bb * vv) * uu + cc * vv * vv;
            let mut dq = aa * (2.0 * uu + 1.0) + bb * vv;
            let inside_row = v >= 0 && v < sh as isize;
            for i in 0..uw {
                let qi = q as i64;
                if (0..1024).contains(&qi) {
                    let wt = self.lut[qi as usize];
                    let x = u + i;
                    if inside_row && x >= 0 && x < sw as isize {
                        let o = v as usize * sw + x as usize;
                        for k in 0..nc {
                            acc[k] += wt * src.at(k, o) as f64;
                        }
                    } else {
                        for a in acc.iter_mut().take(nc) {
                            *a += wt;
                        }
                    }
                    div += wt;
                }
                q += dq;
                dq += ddq;
            }
        }
        if div <= EPS {
            let (u, v) = (u0.round() as isize, v0.round() as isize);
            for (k, o) in out.iter_mut().enumerate() {
                *o = get(u, v, k.min(nc - 1));
            }
            return;
        }
        for (k, o) in out.iter_mut().enumerate() {
            *o = ((acc[k.min(nc - 1)] / div) as f32).clamp(0.0, 1.0);
        }
    }
}

/// Solve a small linear system in place (Gauss-Jordan, partial pivoting).
fn solve(mut m: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let piv = (col..n).max_by(|&i, &j| m[i][col].abs().partial_cmp(&m[j][col].abs()).unwrap())?;
        if m[piv][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, piv);
        b.swap(col, piv);
        let d = m[col][col];
        for j in 0..n {
            m[col][j] /= d;
        }
        b[col] /= d;
        for i in 0..n {
            if i != col {
                let f = m[i][col];
                if f != 0.0 {
                    for j in 0..n {
                        m[i][j] -= f * m[col][j];
                    }
                    b[i] -= f * b[col];
                }
            }
        }
    }
    Some(b)
}

/// `-filter Triangle -virtual-pixel white -define distort:viewport=WxH+0+0
/// -distort Perspective 'sx,sy dx,dy ...'`: the source quad onto the
/// W x H rectangle.
pub fn perspective(src: &dyn Pix, from: [(f64, f64); 4], to: [(f64, f64); 4], w: usize, h: usize) -> Img {
    let mut m = vec![];
    let mut b = vec![];
    for i in 0..4 {
        let (u, v) = from[i];
        let (x, y) = to[i];
        m.push(vec![x, y, 1.0, 0.0, 0.0, 0.0, -x * u, -y * u]);
        b.push(u);
        m.push(vec![0.0, 0.0, 0.0, x, y, 1.0, -x * v, -y * v]);
        b.push(v);
    }
    let c = solve(m, b).unwrap_or_else(|| vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
    let c8 = if c[6] * to[0].0 + c[7] * to[0].1 + 1.0 < 0.0 { -1.0 } else { 1.0 };
    let ewa = Ewa::new(Filter::Triangle);
    let nc = src.nc();
    warp(w, h, nc, |j, row: &mut [&mut [f32]]| {
        let mut px = [0f32; 3];
        for i in 0..w {
            let (dx, dy) = (i as f64 + 0.5, j as f64 + 0.5);
            let p = c[0] * dx + c[1] * dy + c[2];
            let n = c[3] * dx + c[4] * dy + c[5];
            let r = c[6] * dx + c[7] * dy + 1.0;
            if r * c8 < 0.0 {
                for p in row.iter_mut() {
                    p[i] = 0.741;
                }
                continue;
            }
            let s = 1.0 / r;
            let (sx, sy) = (p * s, n * s);
            let s2 = s * s;
            ewa.pixel(
                src,
                sx - 0.5,
                sy - 0.5,
                (r * c[0] - p * c[6]) * s2,
                (r * c[1] - p * c[7]) * s2,
                (r * c[3] - n * c[6]) * s2,
                (r * c[4] - n * c[7]) * s2,
                &mut px[..nc],
            );
            for (k, p) in row.iter_mut().enumerate() {
                p[i] = px[k];
            }
        }
    })
}

/// DeskewImage's turn: `degrees` about the origin on a canvas fitted to the
/// result (bestfit), white outside, Robidoux EWA. Also returns the canvas
/// offset ImageMagick keeps with it (-page), which the next -crop counts from.
pub fn rotate_fit(src: &Img, degrees: f64) -> (Img, isize, isize) {
    let a = (degrees % 360.0).to_radians();
    let (sx, rx, ry, sy) = (a.cos(), a.sin(), -a.sin(), a.cos());
    // forward: x' = sx x + ry y, y' = rx x + sy y
    let fwd = |x: f64, y: f64| (sx * x + ry * y, rx * x + sy * y);
    let (w0, h0) = (src.w as f64, src.h as f64);
    let pts = [fwd(0.0, 0.0), fwd(w0, 0.0), fwd(0.0, h0), fwd(w0, h0)];
    let minx = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min);
    let maxx = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max);
    let miny = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min);
    let maxy = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let gx = (minx - 0.5).floor();
    let gy = (miny - 0.5).floor();
    let w = (maxx - gx + 0.5).ceil() as usize;
    let h = (maxy - gy + 0.5).ceil() as usize;
    // inverse of the forward affine
    let det = sx * sy - ry * rx;
    let (i0, i1, i3, i4) = (sy / det, -ry / det, -rx / det, sx / det);
    let ewa = Ewa::new(Filter::Robidoux);
    let nc = src.c.len();
    let out = warp(w, h, nc, |j, row: &mut [&mut [f32]]| {
        let mut px = [0f32; 3];
        for i in 0..w {
            let (dx, dy) = (gx + i as f64 + 0.5, gy + j as f64 + 0.5);
            let (u, v) = (i0 * dx + i1 * dy, i3 * dx + i4 * dy);
            ewa.pixel(src, u - 0.5, v - 0.5, i0, i1, i3, i4, &mut px[..nc]);
            for (k, p) in row.iter_mut().enumerate() {
                p[i] = px[k];
            }
        }
    });
    (out, gx as isize, gy as isize)
}

/// A new image filled a row at a time: `f(y, [row of each channel])`.
fn warp(w: usize, h: usize, nc: usize, f: impl Fn(usize, &mut [&mut [f32]]) + Sync + Send) -> Img {
    let mut planes: Vec<Plane> = (0..nc).map(|_| Plane::new(w, h)).collect();
    {
        let mut chunks: Vec<std::slice::ChunksMut<f32>> = planes.iter_mut().map(|p| p.d.chunks_mut(w)).collect();
        let rows_: Vec<Vec<&mut [f32]>> = (0..h).map(|_| chunks.iter_mut().map(|c| c.next().unwrap()).collect()).collect();
        #[cfg(feature = "par")]
        {
            use rayon::prelude::*;
            rows_.into_par_iter().enumerate().for_each(|(y, mut r)| f(y, &mut r));
        }
        #[cfg(not(feature = "par"))]
        for (y, mut r) in rows_.into_iter().enumerate() {
            f(y, &mut r);
        }
    }
    Img::from_planes(planes)
}

// ------------------------------------------------------------------ drawing

/// The coverage of a rounded rectangle over the whole of a W x H canvas,
/// `roundrectangle 0,0 W-1,H-1 r,r` antialiased; `stroke` > 0 gives the
/// coverage of its outline drawn that wide instead.
pub fn round_rect(w: usize, h: usize, r: f64, stroke: f64) -> Plane {
    // pixel centres at integers; the shape spans -0.5..W-0.5 when filled,
    // and a stroke is centred on the path through the pixel centres 0..W-1
    let (x0, y0, x1, y1) = if stroke > 0.0 {
        (0.0, 0.0, w as f64 - 1.0, h as f64 - 1.0)
    } else {
        (-0.5, -0.5, w as f64 - 0.5, h as f64 - 0.5)
    };
    let sd = |px: f64, py: f64| -> f64 {
        // signed distance to the rounded rectangle's outline, negative inside
        let cx = px.clamp(x0 + r, x1 - r);
        let cy = py.clamp(y0 + r, y1 - r);
        let (dx, dy) = (px - cx, py - cy);
        let inner = (px - x0).min(x1 - px).min(py - y0).min(y1 - py);
        if (px - cx).abs() > 0.0 && (py - cy).abs() > 0.0 {
            (dx * dx + dy * dy).sqrt() - r
        } else {
            -inner
        }
    };
    let mut o = Plane::new(w, h);
    const S: usize = 4;
    rows(&mut o.d, w, |y, row| {
        for x in 0..w {
            // only the band near the outline needs sampling
            let d0 = sd(x as f64, y as f64);
            let reach = if stroke > 0.0 { stroke / 2.0 + 1.0 } else { 1.0 };
            if stroke > 0.0 && d0.abs() > reach {
                row[x] = 0.0;
                continue;
            }
            if stroke <= 0.0 && d0 < -1.0 {
                row[x] = 1.0;
                continue;
            }
            if stroke <= 0.0 && d0 > 1.0 {
                row[x] = 0.0;
                continue;
            }
            let mut hit = 0;
            for sy in 0..S {
                for sx in 0..S {
                    let px = x as f64 - 0.5 + (sx as f64 + 0.5) / S as f64;
                    let py = y as f64 - 0.5 + (sy as f64 + 0.5) / S as f64;
                    let d = sd(px, py);
                    let inside = if stroke > 0.0 { d.abs() <= stroke / 2.0 } else { d <= 0.0 };
                    hit += inside as usize;
                }
            }
            row[x] = hit as f32 / (S * S) as f32;
        }
    });
    o
}

/// A plain rectangle's coverage, `rectangle x0,y0 x1,y1`: whole pixels.
pub fn rect_mask(w: usize, h: usize, x0: isize, y0: isize, x1: isize, y1: isize) -> Plane {
    let mut o = Plane::new(w, h);
    for y in y0.max(0)..=y1.min(h as isize - 1) {
        for x in x0.max(0)..=x1.min(w as isize - 1) {
            o.d[y as usize * w + x as usize] = 1.0;
        }
    }
    o
}
