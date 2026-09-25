//! flat_field -> neutralize_ink -> tone: the same operators in the same
//! order as the script, over one page held in memory.

use crate::detect::divide;
use crate::img::Img;
use crate::ops::{blur, morph, pct, py_round, resize, rows, sample, stddev, Filter, Plane};
use crate::Opts;

/// With A4DBG=prefix, an intermediate plane as a 16-bit PGM (native only).
#[allow(unused)]
pub fn dbg(name: &str, p: &Plane) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(pre) = std::env::var("A4DBG") {
        let mut b = format!("P5 {} {} 65535\n", p.w, p.h).into_bytes();
        b.extend(p.d.iter().flat_map(|x| ((x.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16).to_be_bytes()));
        std::fs::write(format!("{pre}-{name}.pgm"), b).unwrap();
    }
}

/// Divide by a smoothed per-channel background estimate -> even white paper.
pub fn flat_field(img: &mut Img, o: &Opts) {
    let div = py_round(o.close as f64 / 3.0).max(1) as f64;
    let kern = py_round(o.close as f64 / div).max(1) as usize;
    let (w, h) = (img.w, img.h);
    let bg_scale = o.bg_scale;
    // the percentages as the script prints them into the command line
    let p1: f64 = format!("{:.4}", 100.0 / div).parse().unwrap();
    let p2: f64 = format!("{:.4}", bg_scale * div).parse().unwrap();
    for plane in img.c.iter_mut() {
        let s = sample(plane, pct(w, p1), pct(h, p1));
        let c = morph(&s.d, s.w, s.h, kern, true);
        let c = morph(&c, s.w, s.h, kern, false);
        let c = Plane { w: s.w, h: s.h, d: c };
        let sm = resize(&c, pct(s.w, p2), pct(s.h, p2), Filter::Lanczos);
        let sm = blur(&sm, 6.0);
        let bg = resize(&sm, w, h, Filter::Triangle);
        rows(&mut plane.d, w, |y, row| {
            for (v, &b) in row.iter_mut().zip(bg.row(y)) {
                *v = divide(*v, b);
            }
        });
    }
    img.q8();
}

/// Take the colour cast off the ink, keep genuinely coloured ink. Returns the
/// share of the page kept coloured, in percent.
pub fn neutralize_ink(img: &mut Img, o: &Opts) -> f64 {
    let (w, h) = (img.w, img.h);
    if img.c.len() == 1 {
        *img = img.rgb();
    }
    let gray = img.gray();
    let cthr = crate::ops::pct_thr(o.chroma);
    let dthr = crate::ops::pct_thr(o.ink_dark);
    let mut m = vec![0u8; w * h];
    {
        let [r, g, b] = [&img.c[0], &img.c[1], &img.c[2]];
        rows(&mut m, w, |y, row| {
            let (r, g, b, gr) = (r.row(y), g.row(y), b.row(y), gray.row(y));
            for x in 0..w {
                let mx = r[x].max(g[x]).max(b[x]);
                let mn = r[x].min(g[x]).min(b[x]);
                row[x] = (mx - mn > cthr && gr[x] <= dthr) as u8;
            }
        });
    }
    let m = morph(&m, w, h, 1, false);
    let m = morph(&m, w, h, 1, true);
    // grow and soften at a third of the resolution
    let s = crate::ops::sample_u8(&m, w, h, pct(w, 33.3333), pct(h, 33.3333));
    let grow = py_round(o.chroma_grow as f64 / 3.0).max(1) as usize;
    let s = Plane { w: s.w, h: s.h, d: morph(&s.d, s.w, s.h, grow, true) };
    let s = blur(&s, 1.3333);
    let mask = resize(&s, w, h, Filter::Triangle);
    let share = mask.d.iter().map(|&v| v as f64).sum::<f64>() / (w * h) as f64 * 100.0;
    for p in img.c.iter_mut() {
        rows(&mut p.d, w, |y, row| {
            let (mr, gr) = (mask.row(y), gray.row(y));
            for x in 0..w {
                let a = mr[x];
                row[x] = row[x] * a + gr[x] * (1.0 - a);
            }
        });
    }
    img.q8();
    share
}

/// Whiten where a 0/1 mask is set: `-compose Screen` of the mask.
pub fn whiten(img: &mut Img, m: &[u8]) {
    let w = img.w;
    for p in img.c.iter_mut() {
        rows(&mut p.d, w, |y, row| {
            let m = &m[y * w..(y + 1) * w];
            for x in 0..w {
                if m[x] != 0 {
                    row[x] = 1.0;
                }
            }
        });
    }
}

/// `paper_screen`: paper to pure white, a 1 px ring round the ink kept.
pub fn paper_screen(img: &mut Img, paper_thr: f64) {
    let (w, h) = (img.w, img.h);
    let g = img.gray();
    let thr = crate::ops::pct_thr(paper_thr);
    let m: Vec<u8> = g.d.iter().map(|&v| (v > thr) as u8).collect();
    let m = morph(&m, w, h, 1, false);
    whiten(img, &m);
}

/// Tone, haze and paper-flattening, as the script's tone() does them.
pub fn tone(img: &mut Img, o: &Opts) {
    let (w, h) = (img.w, img.h);
    img.contrast_stretch(o.black_clip, o.white_clip);
    if !o.no_haze {
        let g = img.gray();
        let sd = stddev(&g, 7);
        let (sthr, bthr) = (crate::ops::pct_thr(o.haze_std) * 65535.0, crate::ops::pct_thr(o.haze_min));
        let mut m = vec![0u8; w * h];
        rows(&mut m, w, |y, row| {
            let (s, gr) = (sd.row(y), g.row(y));
            for x in 0..w {
                row[x] = (s[x] * 65535.0 <= sthr && gr[x] > bthr) as u8;
            }
        });
        let m = morph(&m, w, h, 2, false);
        whiten(img, &m);
    }
    if !o.no_flatten_paper {
        paper_screen(img, o.paper_thr);
    }
    img.q8();
}
