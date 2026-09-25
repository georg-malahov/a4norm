//! flat_field -> neutralize_ink -> tone, on a page that is already flat:
//! the same operators in the same order as the Python script, over one page
//! held in memory instead of a file per step.

use crate::ops::{blur, morph, pct, py_round, resize, rows, sample, stddev, Filter, Plane};

pub struct Params {
    pub close: f64,
    pub bg_scale: f64,
    pub chroma: f32,
    pub ink_dark: f32,
    pub chroma_grow: f64,
    pub black_clip: f64,
    pub white_clip: f64,
    pub haze_min: f32,
    pub haze_std: f32,
    pub paper_thr: f32,
    pub haze: bool,
    pub flatten_paper: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            close: 6.0,
            bg_scale: 6.0,
            chroma: 7.0,
            ink_dark: 85.0,
            chroma_grow: 6.0,
            black_clip: 0.1,
            white_clip: 2.0,
            haze_min: 83.0,
            haze_std: 2.0,
            paper_thr: 80.0,
            haze: true,
            flatten_paper: true,
        }
    }
}

/// The page as three planes.
pub struct Rgb {
    pub w: usize,
    pub h: usize,
    pub c: [Plane; 3],
}

const LUMA: [f32; 3] = [0.212656, 0.715158, 0.072186];

impl Rgb {
    pub fn from_rgb8(px: &[u8], w: usize, h: usize) -> Self {
        let mut c = [Plane::new(w, h), Plane::new(w, h), Plane::new(w, h)];
        for (k, p) in c.iter_mut().enumerate() {
            rows(&mut p.d, w, |y, row| {
                let src = &px[y * w * 3..(y + 1) * w * 3];
                for (x, o) in row.iter_mut().enumerate() {
                    *o = src[x * 3 + k] as f32 / 255.0;
                }
            });
        }
        Rgb { w, h, c }
    }

    pub fn to_rgb8(&self, px: &mut [u8]) {
        let w = self.w;
        let c = &self.c;
        rows(px, w * 3, |y, row| {
            for x in 0..w {
                for k in 0..3 {
                    row[x * 3 + k] = (c[k].d[y * w + x] * 255.0 + 0.5) as u8;
                }
            }
        });
    }

    /// `-colorspace gray`: Rec. 709 luma of the stored values.
    pub fn gray(&self) -> Plane {
        let mut g = Plane::new(self.w, self.h);
        let [r, gg, b] = &self.c;
        let w = self.w;
        rows(&mut g.d, w, |y, row| {
            let (r, gg, b) = (r.row(y), gg.row(y), b.row(y));
            for x in 0..w {
                row[x] = LUMA[0] * r[x] + LUMA[1] * gg[x] + LUMA[2] * b[x];
            }
        });
        g
    }

    /// `f(y, r, g, b)` over every row of the three planes.
    fn rows3<F: Fn(usize, &mut [f32], &mut [f32], &mut [f32]) + Sync + Send>(&mut self, f: F) {
        let w = self.w;
        let [r, g, b] = &mut self.c;
        #[cfg(feature = "par")]
        {
            use rayon::prelude::*;
            r.d.par_chunks_mut(w)
                .zip(g.d.par_chunks_mut(w))
                .zip(b.d.par_chunks_mut(w))
                .enumerate()
                .for_each(|(y, ((r, g), b))| f(y, r, g, b));
        }
        #[cfg(not(feature = "par"))]
        for (y, ((r, g), b)) in
            r.d.chunks_mut(w)
                .zip(g.d.chunks_mut(w))
                .zip(b.d.chunks_mut(w))
                .enumerate()
        {
            f(y, r, g, b);
        }
    }

    /// Round to 8 bits, as the script's intermediate files do: they keep the
    /// depth of the photo they came from, so every stage hands the next one
    /// 8-bit values. Near a threshold (the ink chroma test) it matters.
    pub fn quantize8(&mut self) {
        self.rows3(|_, r, g, b| {
            for ch in [r, g, b] {
                for v in ch.iter_mut() {
                    *v = (*v * 255.0 + 0.5).floor() / 255.0;
                }
            }
        });
    }

    /// `-compose Screen` with a 0/1 mask: white where it is set.
    fn whiten(&mut self, m: &[u8]) {
        let w = self.w;
        self.rows3(|y, r, g, b| {
            let m = &m[y * w..(y + 1) * w];
            for x in 0..w {
                if m[x] != 0 {
                    r[x] = 1.0;
                    g[x] = 1.0;
                    b[x] = 1.0;
                }
            }
        });
    }
}

/// Divide by a smoothed per-channel background estimate -> even white paper.
pub fn flat_field(img: &mut Rgb, p: &Params) {
    let div = py_round(p.close / 3.0).max(1) as f64;
    let kern = py_round(p.close / div).max(1) as usize;
    let (w, h) = (img.w, img.h);
    for (k, plane) in img.c.iter_mut().enumerate() {
        let s = sample(plane, pct(w, 100.0 / div), pct(h, 100.0 / div));
        let c = morph(&s.d, s.w, s.h, kern, true);
        let c = morph(&c, s.w, s.h, kern, false);
        let c = Plane {
            w: s.w,
            h: s.h,
            d: c,
        };
        dbg(&format!("f{k}-close"), c.w, c.h, c.d.iter().copied());
        let sm = resize(
            &c,
            pct(s.w, p.bg_scale * div),
            pct(s.h, p.bg_scale * div),
            Filter::Lanczos,
        );
        dbg(&format!("f{k}-small"), sm.w, sm.h, sm.d.iter().copied());
        let sm = blur(&sm, 6.0);
        dbg(&format!("f{k}-blur"), sm.w, sm.h, sm.d.iter().copied());
        let bg = resize(&sm, w, h, Filter::Triangle);
        dbg(&format!("f{k}-bg"), w, h, bg.d.iter().copied());
        rows(&mut plane.d, w, |y, row| {
            for (o, &b) in row.iter_mut().zip(bg.row(y)) {
                *o = divide(*o, b);
            }
        });
        dbg(&format!("f{k}-flat"), w, h, plane.d.iter().copied());
    }
    img.quantize8();
}

/// ImageMagick's Divide for opaque pixels, clamped as Q16 stores it.
#[inline]
fn divide(d: f32, s: f32) -> f32 {
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

/// With A4DBG=prefix, each intermediate mask as a PGM (native only).
#[allow(unused)]
fn dbg(name: &str, w: usize, h: usize, v: impl Iterator<Item = f32>) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(pre) = std::env::var("A4DBG") {
        let mut b = format!("P5 {w} {h} 65535\n").into_bytes();
        b.extend(v.flat_map(|x| ((x.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16).to_be_bytes()));
        std::fs::write(format!("{pre}-{name}.pgm"), b).unwrap();
    }
}

/// Take the colour cast off the ink, keep genuinely coloured ink. Returns the
/// share of the page kept coloured, in percent, as the script reports it.
pub fn neutralize_ink(img: &mut Rgb, p: &Params) -> f64 {
    let (w, h) = (img.w, img.h);
    let gray = img.gray();
    let cthr = p.chroma / 100.0;
    let dthr = p.ink_dark / 100.0;
    // high chroma AND dark
    let mut m = vec![0u8; w * h];
    {
        let [r, g, b] = &img.c;
        rows(&mut m, w, |y, row| {
            let (r, g, b, gr) = (r.row(y), g.row(y), b.row(y), gray.row(y));
            for x in 0..w {
                let mx = r[x].max(g[x]).max(b[x]);
                let mn = r[x].min(g[x]).min(b[x]);
                row[x] = (mx - mn > cthr && gr[x] <= dthr) as u8;
            }
        });
    }
    dbg("m0", w, h, m.iter().map(|&v| v as f32));
    let m = morph(&m, w, h, 1, false);
    let m = morph(&m, w, h, 1, true);
    dbg("m1", w, h, m.iter().map(|&v| v as f32));
    // grow and soften at a third of the resolution
    let mp = Plane {
        w,
        h,
        d: m.iter().map(|&v| v as f32).collect(),
    };
    let s = sample(&mp, pct(w, 33.3333), pct(h, 33.3333));
    let grow = py_round(p.chroma_grow / 3.0).max(1) as usize;
    dbg("m2", s.w, s.h, s.d.iter().copied());
    let s = Plane {
        w: s.w,
        h: s.h,
        d: morph(&s.d, s.w, s.h, grow, true),
    };
    dbg("m3", s.w, s.h, s.d.iter().copied());
    let s = blur(&s, 1.3333);
    dbg("m4", s.w, s.h, s.d.iter().copied());
    let mask = resize(&s, w, h, Filter::Triangle);
    dbg("m5", w, h, mask.d.iter().copied());
    let share = mask.d.iter().map(|&v| v as f64).sum::<f64>() / (w * h) as f64 * 100.0;
    img.rows3(|y, r, g, b| {
        let (mr, gr) = (mask.row(y), gray.row(y));
        for x in 0..w {
            let a = mr[x];
            let k = gr[x] * (1.0 - a);
            r[x] = r[x] * a + k;
            g[x] = g[x] * a + k;
            b[x] = b[x] * a + k;
        }
    });
    img.quantize8();
    share
}

/// `-contrast-stretch B%xW%`: one histogram of the intensity, one map for
/// every channel.
fn contrast_stretch(img: &mut Rgb, black_clip: f64, white_clip: f64) {
    let (w, h) = (img.w, img.h);
    let gray = img.gray();
    let mut hist = vec![0u64; 65536];
    for &v in &gray.d {
        hist[((v * 65535.0 + 0.5) as usize).min(65535)] += 1;
    }
    let n = (w * h) as f64;
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
    img.rows3(|_, r, g, bl| {
        for ch in [r, g, bl] {
            for v in ch.iter_mut() {
                let q = (*v * 65535.0 + 0.5).floor();
                *v = ((q - b) / span).clamp(0.0, 1.0);
            }
        }
    });
}

/// Tone, haze and paper-flattening, as the script's tone() does them.
pub fn tone(img: &mut Rgb, p: &Params) {
    let (w, h) = (img.w, img.h);
    contrast_stretch(img, p.black_clip, p.white_clip);
    if p.haze {
        let g = img.gray();
        let sd = stddev(&g, 7);
        let (sthr, bthr) = (p.haze_std / 100.0 * 65535.0, p.haze_min / 100.0);
        let mut m = vec![0u8; w * h];
        rows(&mut m, w, |y, row| {
            let (s, gr) = (sd.row(y), g.row(y));
            for x in 0..w {
                row[x] = (s[x] * 65535.0 <= sthr && gr[x] > bthr) as u8;
            }
        });
        let m = morph(&m, w, h, 2, false);
        img.whiten(&m);
    }
    if p.flatten_paper {
        let g = img.gray();
        let thr = p.paper_thr / 100.0;
        let m: Vec<u8> = g.d.iter().map(|&v| (v > thr) as u8).collect();
        let m = morph(&m, w, h, 1, false);
        img.whiten(&m);
    }
}

/// The three stages, with the time each took when `clock` is given.
pub fn finish(img: &mut Rgb, p: &Params, clock: Option<&dyn Fn() -> f64>) -> (f64, [f64; 3]) {
    let now = || clock.map_or(0.0, |c| c());
    let t0 = now();
    flat_field(img, p);
    let t1 = now();
    let share = neutralize_ink(img, p);
    let t2 = now();
    tone(img, p);
    let t3 = now();
    (share, [t1 - t0, t2 - t1, t3 - t2])
}
