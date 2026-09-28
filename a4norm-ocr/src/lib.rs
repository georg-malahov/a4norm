//! Text on a page and where it sits: PP-OCRv5 mobile detection and the latin
//! recognizer, run by tract (pure Rust, so the same code builds for the
//! browser). The models are not bundled: the caller hands their bytes in.
//!
//! Detection finds text lines on the page scaled so its long side is
//! `det_long` pixels; recognition reads each line at a height of 48.

use image::{imageops, imageops::FilterType, RgbImage};
use std::io::Cursor;
use tract_onnx::prelude::*;

#[cfg(target_arch = "wasm32")]
mod wasm;

type Plan = std::sync::Arc<TypedRunnableModel>;

/// One recognised line: its text, the recognizer's mean confidence over the
/// characters it kept, and its box in page pixels `[x0, y0, x1, y1]`.
#[derive(Debug, Clone)]
pub struct Line {
    pub text: String,
    pub score: f32,
    pub bbox: [u32; 4],
}

pub struct Ocr {
    det: InferenceModel,
    rec: InferenceModel,
    /// Index 0 is the CTC blank; then the dictionary; then a space.
    chars: Vec<String>,
    det_plans: Vec<((usize, usize), Plan)>,
    rec_plans: Vec<(usize, Plan)>,
    pub det_long: u32,
}

/// Recognition widths: a line is padded to the first that holds it, so a
/// page needs only a few compiled shapes.
const REC_WIDTHS: [usize; 6] = [160, 320, 640, 960, 1280, 1920];
const REC_H: u32 = 48;

impl Ocr {
    /// `dict` is the recognizer's character list, one per line.
    pub fn new(det_onnx: &[u8], rec_onnx: &[u8], dict: &str) -> TractResult<Ocr> {
        let det = tract_onnx::onnx().with_ignore_value_info(true).with_ignore_output_shapes(true).model_for_read(&mut Cursor::new(det_onnx))?;
        let rec = tract_onnx::onnx().with_ignore_value_info(true).with_ignore_output_shapes(true).model_for_read(&mut Cursor::new(rec_onnx))?;
        let mut chars = vec![String::new()];
        chars.extend(dict.lines().map(|l| l.to_string()));
        chars.push(" ".to_string());
        Ok(Ocr { det, rec, chars, det_plans: vec![], rec_plans: vec![], det_long: 960 })
    }

    fn det_plan(&mut self, h: usize, w: usize) -> TractResult<&Plan> {
        if let Some(i) = self.det_plans.iter().position(|(s, _)| *s == (h, w)) {
            return Ok(&self.det_plans[i].1);
        }
        let plan = self
            .det
            .clone()
            .with_input_fact(0, f32::fact([1, 3, h, w]).into())?
            .into_optimized()?
            .into_runnable()?;
        self.det_plans.push(((h, w), plan));
        Ok(&self.det_plans.last().unwrap().1)
    }

    fn rec_plan(&mut self, w: usize) -> TractResult<&Plan> {
        if let Some(i) = self.rec_plans.iter().position(|(s, _)| *s == w) {
            return Ok(&self.rec_plans[i].1);
        }
        let plan = self
            .rec
            .clone()
            .with_input_fact(0, f32::fact([1, 3, REC_H as usize, w]).into())?
            .into_optimized()?
            .into_runnable()?;
        self.rec_plans.push((w, plan));
        Ok(&self.rec_plans.last().unwrap().1)
    }

    /// Text line boxes in page pixels.
    /// Text line boxes in page pixels, and the page's skew in radians (the
    /// median slope of its long lines).
    pub fn detect(&mut self, page: &RgbImage) -> TractResult<(Vec<[u32; 4]>, f32)> {
        let (pw, ph) = page.dimensions();
        let s = self.det_long as f32 / pw.max(ph) as f32;
        let r32 = |v: f32| (((v / 32.0).round() as usize).max(1)) * 32;
        let (w, h) = (r32(pw as f32 * s), r32(ph as f32 * s));
        let small = imageops::resize(page, w as u32, h as u32, FilterType::Triangle);
        // BGR, (x/255 - mean) / std, channels first
        let (mean, std) = ([0.406f32, 0.456, 0.485], [0.225f32, 0.224, 0.229]);
        let mut data = vec![0f32; 3 * h * w];
        for (x, y, p) in small.enumerate_pixels() {
            let i = y as usize * w + x as usize;
            for c in 0..3 {
                let v = p.0[2 - c] as f32 / 255.0;
                data[c * h * w + i] = (v - mean[c]) / std[c];
            }
        }
        let input = Tensor::from_shape(&[1, 3, h, w], &data)?;
        let out = self.det_plan(h, w)?.run(tvec!(input.into()))?;
        let prob = out[0].to_plain_array_view::<f32>()?;
        let prob: Vec<f32> = prob.iter().copied().collect(); // [1,1,h,w]
        let (boxes, mut slopes) = db_boxes(&prob, w, h, 0.3, 0.6, 1.5);
        slopes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let skew = if slopes.is_empty() { 0.0 } else { slopes[slopes.len() / 2].atan() };
        let (sx, sy) = (pw as f32 / w as f32, ph as f32 / h as f32);
        Ok((boxes
            .into_iter()
            .map(|[x0, y0, x1, y1]| {
                [
                    (x0 * sx).max(0.0) as u32,
                    (y0 * sy).max(0.0) as u32,
                    ((x1 * sx) as u32).min(pw),
                    ((y1 * sy) as u32).min(ph),
                ]
            })
            .collect(), skew))
    }

    /// Reads one line cut from the page.
    pub fn read(&mut self, crop: &RgbImage) -> TractResult<(String, f32)> {
        let (cw, ch) = crop.dimensions();
        let tw = ((REC_H as f32 * cw as f32 / ch.max(1) as f32).ceil() as usize).max(8);
        let bucket = *REC_WIDTHS.iter().find(|&&b| b >= tw).unwrap_or(&REC_WIDTHS[5]);
        let tw = tw.min(bucket);
        let img = imageops::resize(crop, tw as u32, REC_H, FilterType::Triangle);
        let (h, w) = (REC_H as usize, bucket);
        let mut data = vec![0f32; 3 * h * w]; // right padding: 0 after normalizing
        for (x, y, p) in img.enumerate_pixels() {
            let i = y as usize * w + x as usize;
            for c in 0..3 {
                data[c * h * w + i] = (p.0[2 - c] as f32 / 255.0 - 0.5) / 0.5;
            }
        }
        let input = Tensor::from_shape(&[1, 3, h, w], &data)?;
        let out = self.rec_plan(w)?.run(tvec!(input.into()))?;
        let a = out[0].to_plain_array_view::<f32>()?; // [1, T, C]
        let (t_len, classes) = (a.shape()[1], a.shape()[2]);
        let flat: Vec<f32> = a.iter().copied().collect();
        let (mut text, mut sum, mut n, mut prev) = (String::new(), 0f32, 0usize, 0usize);
        for t in 0..t_len {
            let row = &flat[t * classes..(t + 1) * classes];
            let (k, &p) = row
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .unwrap();
            if k != 0 && k != prev {
                if k < self.chars.len() {
                    text.push_str(self.chars[k].as_str());
                    sum += p;
                    n += 1;
                }
            }
            prev = k;
        }
        Ok((text.trim().to_string(), if n > 0 { sum / n as f32 } else { 0.0 }))
    }

    /// Every text line on the page, top to bottom, with the skew the page
    /// was levelled by first (radians; boxes are in the levelled page).
    pub fn page(&mut self, page: &RgbImage) -> TractResult<(Vec<Line>, f32)> {
        let (mut boxes, skew) = self.detect(page)?;
        let levelled;
        let page = if skew.abs() > 0.1f32.to_radians() {
            levelled = rotate(page, skew);
            boxes = self.detect(&levelled)?.0;
            &levelled
        } else {
            page
        };
        boxes.sort_by_key(|b| (b[1] / 8, b[0]));
        let mut lines = Vec::with_capacity(boxes.len());
        for b in boxes {
            let (w, h) = (b[2].saturating_sub(b[0]), b[3].saturating_sub(b[1]));
            if w < 4 || h < 4 {
                continue;
            }
            let crop = imageops::crop_imm(page, b[0], b[1], w, h).to_image();
            let (text, score) = self.read(&crop)?;
            if !text.is_empty() {
                lines.push(Line { text, score, bbox: b });
            }
        }
        Ok((lines, skew))
    }
}

/// DB post-processing on the probability map, with axis-aligned boxes (the
/// page arrives straightened): threshold, 4-connected regions, the region's
/// mean probability as its score, then the box grown by `unclip`.
fn db_boxes(prob: &[f32], w: usize, h: usize, thresh: f32, box_thresh: f32, unclip: f32) -> (Vec<[f32; 4]>, Vec<f32>) {
    let mut label = vec![0u32; w * h];
    let mut out = vec![];
    let mut slopes = vec![];
    let mut stack = vec![];
    let mut next = 0u32;
    for start in 0..w * h {
        if prob[start] <= thresh || label[start] != 0 {
            continue;
        }
        next += 1;
        label[start] = next;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        let (mut sum, mut n) = (0f32, 0usize);
        let (mut sx, mut sy, mut sxx, mut sxy) = (0f64, 0f64, 0f64, 0f64);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            let (fx, fy) = (x as f64, y as f64);
            sx += fx;
            sy += fy;
            sxx += fx * fx;
            sxy += fx * fy;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            sum += prob[i];
            n += 1;
            let mut push = |j: usize| {
                if prob[j] > thresh && label[j] == 0 {
                    label[j] = next;
                    stack.push(j);
                }
            };
            if x > 0 { push(i - 1); }
            if x + 1 < w { push(i + 1); }
            if y > 0 { push(i - w); }
            if y + 1 < h { push(i + w); }
        }
        let (bw, bh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
        if bw.min(bh) < 3.0 || sum / (n as f32) < box_thresh {
            continue;
        }
        if bw > 8.0 * bh && n > 200 {
            let nf = n as f64;
            let var = sxx / nf - (sx / nf).powi(2);
            if var > 0.0 {
                slopes.push(((sxy / nf - sx / nf * sy / nf) / var) as f32);
            }
        }
        let d = bw * bh * unclip / (2.0 * (bw + bh));
        out.push([x0 as f32 - d, y0 as f32 - d, x1 as f32 + 1.0 + d, y1 as f32 + 1.0 + d]);
    }
    (out, slopes)
}

/// The page turned by `angle` radians about its centre (bilinear, white
/// outside), so that lines tilted by `angle` become level.
pub fn rotate(page: &RgbImage, angle: f32) -> RgbImage {
    let (w, h) = page.dimensions();
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let (s, c) = angle.sin_cos();
    let mut out = RgbImage::from_pixel(w, h, image::Rgb([255, 255, 255]));
    for y in 0..h {
        for x in 0..w {
            // where this output pixel comes from in the tilted page
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            let (sx, sy) = (cx + dx * c - dy * s, cy + dx * s + dy * c);
            if sx < 0.0 || sy < 0.0 || sx >= (w - 1) as f32 || sy >= (h - 1) as f32 {
                continue;
            }
            let (x0, y0) = (sx as u32, sy as u32);
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            let p = |xx, yy| page.get_pixel(xx, yy).0;
            let (a, b, cc, d) = (p(x0, y0), p(x0 + 1, y0), p(x0, y0 + 1), p(x0 + 1, y0 + 1));
            let mut v = [0u8; 3];
            for k in 0..3 {
                let top = a[k] as f32 * (1.0 - fx) + b[k] as f32 * fx;
                let bot = cc[k] as f32 * (1.0 - fx) + d[k] as f32 * fx;
                v[k] = (top * (1.0 - fy) + bot * fy).round() as u8;
            }
            out.put_pixel(x, y, image::Rgb(v));
        }
    }
    out
}
