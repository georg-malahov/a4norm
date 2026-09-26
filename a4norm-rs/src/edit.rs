//! Edits of a finished page, for the browser's Edit panel and its size
//! control: a smart eraser, and the page written again smaller.

use crate::img::{self, Img, Src};
use crate::io;
use crate::ops::{self, Plane};
use crate::Fail;

/// The dpi in a JPEG's JFIF header, or None when it has none in dots per
/// inch or per centimetre.
pub fn jfif_dpi(data: &[u8]) -> Option<usize> {
    let mut i = 2;
    while i + 4 < data.len() && data[i] == 0xFF {
        let m = data[i + 1];
        let len = ((data[i + 2] as usize) << 8) | data[i + 3] as usize;
        if m == 0xE0 && data.get(i + 4..i + 9) == Some(b"JFIF\0") && i + 16 <= data.len() {
            let x = ((data[i + 12] as usize) << 8) | data[i + 13] as usize;
            return match data[i + 11] {
                1 if x > 0 => Some(x),
                2 if x > 0 => Some((x as f64 * 2.54).round() as usize),
                _ => None,
            };
        }
        if m == 0xDA {
            break;
        }
        i += 2 + len;
    }
    None
}

/// Fill round spots of a page from what surrounds them: a smooth membrane
/// through the colours at their rim, so a finger, a stain or a shadow gives
/// way to the page's own tone, whatever it is (white, a passport's grey or
/// blue), shading included. `dots` are (x, y, r) in page pixels.
pub fn inpaint(page: &mut Img, dots: &[(f64, f64, f64)]) {
    let (w, h) = (page.w, page.h);
    if dots.is_empty() {
        return;
    }
    // the work stays in a box round the spots, a margin wide for the rim
    let rmax = dots.iter().map(|d| d.2).fold(0.0, f64::max);
    let pad = rmax.max(8.0) + 4.0;
    let x0 = dots.iter().map(|d| d.0 - d.2 - pad).fold(f64::MAX, f64::min).max(0.0) as usize;
    let y0 = dots.iter().map(|d| d.1 - d.2 - pad).fold(f64::MAX, f64::min).max(0.0) as usize;
    let x1 = (dots.iter().map(|d| d.0 + d.2 + pad).fold(f64::MIN, f64::max).ceil() as usize).min(w);
    let y1 = (dots.iter().map(|d| d.1 + d.2 + pad).fold(f64::MIN, f64::max).ceil() as usize).min(h);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (bw, bh) = (x1 - x0, y1 - y0);
    // the hole: every spot a pixel wider, so the brush's soft rim goes too
    let mut hole = Plane::new(bw, bh);
    for &(cx, cy, r) in dots {
        let r = r + 1.0;
        let (lx, ly) = ((cx - r - x0 as f64).floor().max(0.0) as usize, (cy - r - y0 as f64).floor().max(0.0) as usize);
        let (hx, hy) = (((cx + r - x0 as f64).ceil() as usize + 1).min(bw), ((cy + r - y0 as f64).ceil() as usize + 1).min(bh));
        for y in ly..hy {
            for x in lx..hx {
                let (dx, dy) = (x as f64 + x0 as f64 + 0.5 - cx, y as f64 + y0 as f64 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    hole.d[y * bw + x] = 1.0;
                }
            }
        }
    }
    // blended in over a pixel or two, so the fill has no hard rim
    let alpha = ops::blur(&hole, 1.0).zip(&hole, |b, m| b.max(m));
    for p in page.c.iter_mut() {
        let mut v = Plane::new(bw, bh);
        for y in 0..bh {
            v.d[y * bw..(y + 1) * bw].copy_from_slice(&p.d[(y0 + y) * w + x0..(y0 + y) * w + x1]);
        }
        let known: Vec<f32> = hole.d.iter().map(|&m| 1.0 - m).collect();
        let fill = push_pull(&v.d, &known, bw, bh);
        for y in 0..bh {
            let row = &mut p.d[(y0 + y) * w + x0..(y0 + y) * w + x1];
            for x in 0..bw {
                let a = alpha.d[y * bw + x];
                row[x] = fill[y * bw + x] * a + row[x] * (1.0 - a);
            }
        }
    }
    page.q8();
}

/// A page's JPEG with spots filled (see inpaint), written again at
/// `quality` with its dpi kept.
pub fn inpaint_jpeg(jpg: &[u8], dots: &[(f64, f64, f64)], quality: u8) -> Result<Vec<u8>, Fail> {
    let mut page = io::decode(jpg, "page")?.to_img();
    inpaint(&mut page, dots);
    Ok(io::encode_jpeg(&page, quality, false, jfif_dpi(jpg).unwrap_or(200)))
}

/// Push-pull: the known pixels (weight 1) averaged down a pyramid until the
/// holes close, then brought back up, each level filling its holes from the
/// smooth level below it.
fn push_pull(v: &[f32], k: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut levels = vec![(w, h, v.to_vec(), k.to_vec())];
    while levels.last().unwrap().3.iter().any(|&x| x < 1.0) {
        let (lw, lh, lv, lk) = levels.last().unwrap();
        let (lw, lh) = (*lw, *lh);
        if lw == 1 && lh == 1 {
            break;
        }
        let (nw, nh) = (lw.div_ceil(2), lh.div_ceil(2));
        let (mut nv, mut nk) = (vec![0.0; nw * nh], vec![0.0; nw * nh]);
        for y in 0..nh {
            for x in 0..nw {
                let (mut sv, mut sk) = (0.0, 0.0);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (xx, yy) = (2 * x + dx, 2 * y + dy);
                    if xx < lw && yy < lh {
                        let i = yy * lw + xx;
                        sv += lv[i] * lk[i];
                        sk += lk[i];
                    }
                }
                nv[y * nw + x] = if sk > 0.0 { sv / sk } else { 0.0 };
                nk[y * nw + x] = sk.min(1.0);
            }
        }
        levels.push((nw, nh, nv, nk));
    }
    for l in (0..levels.len() - 1).rev() {
        let (cw, ch, cv) = {
            let c = &levels[l + 1];
            (c.0, c.1, c.2.clone())
        };
        let (lw, lh, lv, lk) = &mut levels[l];
        for y in 0..*lh {
            for x in 0..*lw {
                let i = y * *lw + x;
                if lk[i] >= 1.0 {
                    continue;
                }
                // bilinear from the coarser level, pixel centres aligned
                let fx = ((x as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (cw - 1) as f32);
                let fy = ((y as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (ch - 1) as f32);
                let (ax, ay) = (fx.floor() as usize, fy.floor() as usize);
                let (bx, by) = ((ax + 1).min(cw - 1), (ay + 1).min(ch - 1));
                let (tx, ty) = (fx - ax as f32, fy - ay as f32);
                let c = cv[ay * cw + ax] * (1.0 - tx) * (1.0 - ty)
                    + cv[ay * cw + bx] * tx * (1.0 - ty)
                    + cv[by * cw + ax] * (1.0 - tx) * ty
                    + cv[by * cw + bx] * tx * ty;
                lv[i] = lk[i] * lv[i] + (1.0 - lk[i]) * c;
            }
        }
    }
    levels.swap_remove(0).2
}

/// A finished page written again: brought down to `dpi` when it is higher,
/// made grey when asked, at JPEG `quality`. Returns the JPEG and its dpi.
/// Colour is subsampled 4:2:0 once the page leaves today's quality or
/// resolution: at that point the file size is what is asked for.
pub fn recompress(jpg: &[u8], dpi: usize, quality: u8, gray: bool) -> Result<(Vec<u8>, usize), Fail> {
    let src = io::decode(jpg, "page")?;
    let cur = jfif_dpi(jpg).unwrap_or(dpi);
    let to = dpi.min(cur).max(1);
    let (w2, h2) = if to < cur {
        (ops::py_round(src.w as f64 * to as f64 / cur as f64).max(1) as usize, ops::py_round(src.h as f64 * to as f64 / cur as f64).max(1) as usize)
    } else {
        (src.w, src.h)
    };
    let grey_page = gray || is_grey(&src);
    let out = if grey_page {
        let g = if (w2, h2) == (src.w, src.h) { img::gray_resized(&src, w2, h2) } else { gray_small(&src, w2, h2) };
        io::encode_jpeg(&Img::from_planes(vec![g]), quality, false, to)
    } else {
        let sub = quality < crate::Opts::default().quality || to < cur;
        if (w2, h2) == (src.w, src.h) {
            io::encode_rgb8(&src.px, src.w, src.h, quality, sub, to)
        } else {
            // a triangle is plenty for a page going down by a quarter, and
            // a third of Lanczos's work
            let small = img::resize_with(&src, w2, h2, ops::Filter::Triangle).to_rgb8();
            io::encode_rgb8(&small, w2, h2, quality, sub, to)
        }
    };
    Ok((out, to))
}

/// The page's grey, brought down through a triangle.
fn gray_small(src: &Src, w2: usize, h2: usize) -> Plane {
    let g = Img::from_planes(vec![img::gray_resized(src, src.w, src.h)]);
    img::resize_with(&g, w2, h2, ops::Filter::Triangle).c.swap_remove(0)
}

/// A finished page turned by `quarters` quarter turns clockwise and written
/// again at `quality`, its dpi kept, a grey page grey.
pub fn rotate(jpg: &[u8], quarters: i32, quality: u8) -> Result<Vec<u8>, Fail> {
    use image::metadata::Orientation::*;
    let src = io::decode(jpg, "page")?;
    let turned = match quarters.rem_euclid(4) {
        1 => io::orient_px(src, Rotate90),
        2 => io::orient_px(src, Rotate180),
        3 => io::orient_px(src, Rotate270),
        _ => src,
    };
    let dpi = jfif_dpi(jpg).unwrap_or(200);
    Ok(if is_grey(&turned) {
        let g: Vec<u8> = turned.px.iter().step_by(3).copied().collect();
        io::encode_luma8(&g, turned.w, turned.h, quality, dpi)
    } else {
        io::encode_rgb8(&turned.px, turned.w, turned.h, quality, false, dpi)
    })
}

/// Every pixel neutral: a page written grey, read back as RGB.
fn is_grey(s: &Src) -> bool {
    s.px.as_chunks::<3>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eraser_takes_the_tone_around() {
        // a blue-grey page, lighter at the top, with a dark blot in it
        let (w, h) = (120, 100);
        let mut planes = vec![];
        for base in [0.55f32, 0.65, 0.80] {
            let mut p = Plane::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let d = ((x as f32 - 60.0).powi(2) + (y as f32 - 50.0).powi(2)).sqrt();
                    let tone = base + 0.1 * (1.0 - y as f32 / h as f32);
                    p.d[y * w + x] = if d < 12.0 { 0.1 } else { tone };
                }
            }
            planes.push(p);
        }
        let mut page = Img::from_planes(planes);
        inpaint(&mut page, &[(60.0, 50.0, 13.0)]);
        for (k, base) in [0.55f32, 0.65, 0.80].iter().enumerate() {
            let v = page.c[k].d[50 * w + 60];
            let want = base + 0.1 * (1.0 - 50.0 / h as f32);
            assert!((v - want).abs() < 0.03, "channel {k}: {v} for {want}");
        }
        // and the page away from the spot is untouched
        assert!((page.c[0].d[5 * w + 5] - (0.55 + 0.1 * (1.0 - 5.0 / h as f32))).abs() < 0.005);
    }

    #[test]
    fn a_quarter_turn_is_clockwise() {
        // 40x20, dark at the top-left: after a quarter turn clockwise the
        // page is 20x40 and the dark corner is at its top-right
        let (w, h) = (40, 20);
        let px: Vec<u8> = (0..w * h).flat_map(|i| if i % w < 10 && i / w < 10 { [0u8; 3] } else { [255u8; 3] }).collect();
        let jpg = io::encode_rgb8(&px, w, h, 95, false, 150);
        let out = rotate(&jpg, 1, 95).unwrap();
        assert_eq!(jfif_dpi(&out), Some(150));
        let s = io::decode(&out, "p").unwrap();
        assert_eq!((s.w, s.h), (20, 40));
        assert!(s.px[3 * (2 * 20 + 17)] < 60, "top-right should be dark");
        assert!(s.px[3 * (2 * 20 + 2)] > 200, "top-left should be light");
    }

    #[test]
    fn smaller_page_keeps_its_dpi_in_the_header() {
        let (w, h) = (200, 280);
        let px: Vec<u8> = (0..w * h * 3).map(|i| if (i / 3) % 17 < 3 { 30 } else { 240 }).collect();
        let jpg = io::encode_rgb8(&px, w, h, 88, false, 200);
        assert_eq!(jfif_dpi(&jpg), Some(200));
        let (out, dpi) = recompress(&jpg, 150, 60, false).unwrap();
        assert_eq!((dpi, jfif_dpi(&out)), (150, Some(150)));
        let s = io::decode(&out, "p").unwrap();
        assert_eq!((s.w, s.h), (150, 210));
        assert!(out.len() < jpg.len());
        // a higher dpi than the page has is not invented
        assert_eq!(recompress(&jpg, 300, 88, false).unwrap().1, 200);
    }
}
