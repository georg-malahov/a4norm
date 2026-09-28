//! Text line detection: PP-OCRv5 mobile det on the page scaled to a long side
//! of `long` pixels, then DB post-processing into boxes that follow the
//! page's skew.
//!
//! The skew is the median slope of the page's long regions. Every box is the
//! extent of its region in the levelled frame, so a long line of small print
//! on a tilted page gets a thin tilted box instead of an upright one that
//! takes in the lines above and below.

use crate::Plan;
use image::{imageops, imageops::FilterType, RgbImage};
use tract_onnx::prelude::*;

/// The model's own post-processing settings.
const THRESH: f32 = 0.3;
const BOX_THRESH: f32 = 0.6;
const UNCLIP: f32 = 1.5;

/// A text line's box in the levelled frame, in page pixels: `u` along the
/// lines, `v` across them (see [`Frame`]).
#[derive(Debug, Clone, Copy)]
pub struct TextBox {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

/// The levelled frame of a page tilted by `angle` radians: `u = x cos + y sin`,
/// `v = -x sin + y cos`, so a line tilted by `angle` has one `v`.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub cos: f32,
    pub sin: f32,
}

impl Frame {
    pub fn new(angle: f32) -> Frame {
        let (sin, cos) = angle.sin_cos();
        Frame { cos, sin }
    }
    pub fn level(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.cos + y * self.sin, -x * self.sin + y * self.cos)
    }
    pub fn page(&self, u: f32, v: f32) -> (f32, f32) {
        (u * self.cos - v * self.sin, u * self.sin + v * self.cos)
    }
    /// The upright box in page pixels around a levelled box.
    pub fn bbox(&self, u0: f32, v0: f32, u1: f32, v1: f32) -> [f32; 4] {
        let c = [self.page(u0, v0), self.page(u1, v0), self.page(u0, v1), self.page(u1, v1)];
        let (mut b0, mut b1) = ([f32::MAX; 2], [f32::MIN; 2]);
        for (x, y) in c {
            b0 = [b0[0].min(x), b0[1].min(y)];
            b1 = [b1[0].max(x), b1[1].max(y)];
        }
        [b0[0], b0[1], b1[0], b1[1]]
    }
}

/// The input shape for a page of `pw` x `ph`: the long side `long`, both
/// sides multiples of 32.
pub fn shape(pw: u32, ph: u32, long: u32) -> (usize, usize) {
    let s = long as f32 / pw.max(ph) as f32;
    let r32 = |v: f32| (((v / 32.0).round() as usize).max(1)) * 32;
    (r32(ph as f32 * s), r32(pw as f32 * s))
}

/// Text line boxes in the levelled frame and the page's skew in radians.
pub fn detect(plan: &Plan, page: &RgbImage, (h, w): (usize, usize)) -> TractResult<(Vec<TextBox>, f32)> {
    let (pw, ph) = page.dimensions();
    let small = imageops::resize(page, w as u32, h as u32, FilterType::Triangle);
    // BGR, (x/255 - mean) / std, channels first
    let (mean, std) = ([0.406f32, 0.456, 0.485], [0.225f32, 0.224, 0.229]);
    let mut data = vec![0f32; 3 * h * w];
    for (x, y, p) in small.enumerate_pixels() {
        let i = y as usize * w + x as usize;
        for c in 0..3 {
            data[c * h * w + i] = (p.0[2 - c] as f32 / 255.0 - mean[c]) / std[c];
        }
    }
    let out = plan.run(tvec!(Tensor::from_shape(&[1, 3, h, w], &data)?.into()))?;
    let prob = out[0].to_plain_array_view::<f32>()?;
    let prob = prob.as_slice().expect("contiguous output"); // [1, 1, h, w]
    let regions = regions(prob, w, h);
    let mut slopes: Vec<f32> = regions.iter().filter_map(|r| r.slope).collect();
    slopes.sort_by(f32::total_cmp);
    let skew = if slopes.is_empty() { 0.0 } else { slopes[slopes.len() / 2].atan() };
    // Map scale: the map is the page scaled by (w/pw, h/ph), near enough one
    // factor; the frame is set in page pixels.
    let (sx, sy) = (pw as f32 / w as f32, ph as f32 / h as f32);
    let f = Frame::new(skew);
    let cores: Vec<TextBox> = regions
        .iter()
        .map(|r| {
            let (mut u0, mut v0, mut u1, mut v1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for &(x, y) in &r.px {
                // the pixel's four corners, in page pixels
                for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                    let (u, v) = f.level((x as f32 + dx) * sx, (y as f32 + dy) * sy);
                    (u0, v0, u1, v1) = (u0.min(u), v0.min(v), u1.max(u), v1.max(v));
                }
            }
            TextBox { u0, v0, u1, v1 }
        })
        .collect();
    Ok((unclip(&cores), skew))
}

/// The text around each region: the region is the text shrunk the way the
/// model was trained, grown back by area * ratio / perimeter. Where lines
/// are set tight the grown boxes overlap, and a line cut out with a strip of
/// its neighbour reads as garbage: two boxes side by side across the lines
/// then part halfway between their regions.
fn unclip(cores: &[TextBox]) -> Vec<TextBox> {
    let mut out: Vec<TextBox> = cores
        .iter()
        .map(|c| {
            let (bw, bh) = (c.u1 - c.u0, c.v1 - c.v0);
            let d = bw * bh * UNCLIP / (2.0 * (bw + bh));
            TextBox { u0: c.u0 - d, v0: c.v0 - d, u1: c.u1 + d, v1: c.v1 + d }
        })
        .collect();
    for i in 0..cores.len() {
        for j in 0..cores.len() {
            let (a, b) = (&cores[i], &cores[j]);
            // b below a, and the two share some of their length
            if b.v0 < a.v1 || a.u1.min(b.u1) <= a.u0.max(b.u0) {
                continue;
            }
            let mid = (a.v1 + b.v0) / 2.0;
            out[i].v1 = out[i].v1.min(mid);
            out[j].v0 = out[j].v0.max(mid);
        }
    }
    out
}

struct Region {
    px: Vec<(u32, u32)>,
    /// dy/dx of a long thin region, by least squares
    slope: Option<f32>,
}

/// 4-connected regions above THRESH whose mean probability passes BOX_THRESH.
fn regions(prob: &[f32], w: usize, h: usize) -> Vec<Region> {
    let mut seen = vec![false; w * h];
    let mut out = vec![];
    let mut stack = vec![];
    for start in 0..w * h {
        if prob[start] <= THRESH || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut px = vec![];
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        let mut sum = 0f32;
        let (mut sx, mut sy, mut sxx, mut sxy) = (0f64, 0f64, 0f64, 0f64);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            px.push((x as u32, y as u32));
            let (fx, fy) = (x as f64, y as f64);
            (sx, sy, sxx, sxy) = (sx + fx, sy + fy, sxx + fx * fx, sxy + fx * fy);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            sum += prob[i];
            let mut push = |j: usize| {
                if prob[j] > THRESH && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < w {
                push(i + 1);
            }
            if y > 0 {
                push(i - w);
            }
            if y + 1 < h {
                push(i + w);
            }
        }
        let n = px.len();
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        if bw.min(bh) < 3 || sum / (n as f32) < BOX_THRESH {
            continue;
        }
        let nf = n as f64;
        let var = sxx / nf - (sx / nf).powi(2);
        let slope = (bw > 8 * bh && n > 200 && var > 0.0).then(|| ((sxy / nf - sx / nf * sy / nf) / var) as f32);
        out.push(Region { px, slope });
    }
    out
}
