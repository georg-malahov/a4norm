//! Line recognition: the latin PP-OCRv5 mobile recognizer on a line cut out
//! of the page along its tilted box, scaled to a height of 48, then greedy
//! CTC decoding that keeps where each character was, so the line splits
//! into words with their own boxes.

use crate::det::{Frame, TextBox};
use crate::Plan;
use image::RgbImage;
use tract_onnx::prelude::*;

pub const H: usize = 48;
/// The recognizer's time step is 8 input columns.
const STEP: usize = 8;

/// Input widths a line is padded to, about 1.25 apart: each is a compiled
/// plan (~60 ms natively), and a line pays for its padding. A line wider
/// than the last is squeezed into it.
pub const WIDTHS: [usize; 15] = [64, 96, 128, 160, 192, 256, 320, 384, 480, 608, 768, 960, 1216, 1536, 1920];

/// A line cut out and scaled: `data` is the recognizer's input, `tw` the
/// columns that hold the line (the rest is padding), `scale` page pixels
/// per input column along the line.
pub struct Cut {
    pub data: Vec<f32>,
    pub width: usize,
    pub tw: usize,
    pub scale: f32,
}

/// The line under `b` sampled from the page (bilinear), height 48, in the
/// recognizer's normalization; the width rounded up to one of WIDTHS.
/// `flip` samples it turned half round, as it reads on a page upside down.
pub fn cut(page: &RgbImage, f: &Frame, b: &TextBox, flip: bool) -> Cut {
    sample(page, b.u1 - b.u0, b.v1 - b.v0, |a, c| if flip { f.page(b.u1 - a, b.v1 - c) } else { f.page(b.u0 + a, b.v0 + c) })
}

/// The line in `b` (`[x0, y0, x1, y1]` in page pixels) as it reads on the
/// page turned `turn` degrees clockwise: at 90 it runs up the page, its
/// letters' tops to the left; at 270 down, their tops to the right.
pub fn cut_turned(page: &RgbImage, b: [f32; 4], turn: u16) -> Cut {
    let [x0, y0, x1, y1] = b;
    let (w, h) = (x1 - x0, y1 - y0);
    match turn {
        90 => sample(page, h, w, |a, c| (x0 + c, y1 - a)),
        180 => sample(page, w, h, |a, c| (x1 - a, y1 - c)),
        270 => sample(page, h, w, |a, c| (x1 - c, y0 + a)),
        _ => sample(page, w, h, |a, c| (x0 + a, y0 + c)),
    }
}

/// The recognizer's width for a line `along` long and `across` high.
pub fn width(along: f32, across: f32) -> usize {
    let want = ((H as f32 * along / across).ceil() as usize).max(8);
    *WIDTHS.iter().find(|&&w| w >= want).unwrap_or(&WIDTHS[WIDTHS.len() - 1])
}

/// A line `along` x `across` page pixels, `at(a, c)` the page point `a`
/// along it and `c` across from its start and the top of its letters.
fn sample(page: &RgbImage, along: f32, across: f32, at: impl Fn(f32, f32) -> (f32, f32)) -> Cut {
    let want = ((H as f32 * along / across).ceil() as usize).max(8);
    let width = width(along, across);
    let tw = want.min(width);
    let (su, sv) = (along / tw as f32, across / H as f32);
    let (pw, ph) = page.dimensions();
    let raw = page.as_raw();
    let mut data = vec![0f32; 3 * H * width]; // padding: 0 after normalizing
    for j in 0..H {
        let c = (j as f32 + 0.5) * sv;
        for i in 0..tw {
            let (x, y) = at((i as f32 + 0.5) * su, c);
            let rgb = bilinear(raw, pw, ph, x - 0.5, y - 0.5);
            for k in 0..3 {
                // BGR, (x/255 - 0.5) / 0.5
                data[k * H * width + j * width + i] = rgb[2 - k] / 127.5 - 1.0;
            }
        }
    }
    Cut { data, width, tw, scale: su }
}

fn bilinear(raw: &[u8], w: u32, h: u32, x: f32, y: f32) -> [f32; 3] {
    let (w, h) = (w as i64, h as i64);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |xx: i64, yy: i64| -> [f32; 3] {
        // white outside the page
        if xx < 0 || yy < 0 || xx >= w || yy >= h {
            return [255.0; 3];
        }
        let i = ((yy * w + xx) * 3) as usize;
        [raw[i] as f32, raw[i + 1] as f32, raw[i + 2] as f32]
    };
    let (x0, y0) = (x0 as i64, y0 as i64);
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    let mut out = [0f32; 3];
    for k in 0..3 {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bot = c[k] + (d[k] - c[k]) * fx;
        out[k] = top + (bot - top) * fy;
    }
    out
}

/// A word of a line: its text, mean confidence, and where it runs along
/// the line in page pixels from the box's start.
pub struct Span {
    pub text: String,
    pub score: f32,
    pub u0: f32,
    pub u1: f32,
    /// where each character was read, along the line like `u0`
    pub at: Vec<f32>,
}

/// Reads a cut line: its words, split at the recognizer's spaces.
/// `chars[0]` is the CTC blank; the last is the space.
pub fn read(plan: &Plan, c: &Cut, chars: &[String]) -> TractResult<Vec<Span>> {
    let input = Tensor::from_shape(&[1, 3, H, c.width], &c.data)?;
    let out = plan.run(tvec!(input.into()))?;
    let a = out[0].to_plain_array_view::<f32>()?; // [1, T, C]
    let (t_len, classes) = (a.shape()[1], a.shape()[2]);
    let flat = a.as_slice().expect("contiguous output");
    // frames past the line are padding
    let t_len = t_len.min(c.tw.div_ceil(STEP));
    let space = chars.len() - 1;
    let mut words: Vec<Span> = vec![];
    let mut cur: Option<(String, f32, usize, usize, usize)> = None; // text, sum, n, first, last frame
    let mut at: Vec<f32> = vec![];
    let mut prev = 0usize;
    let col = |t: usize| (t * STEP) as f32 * c.scale;
    // A character is emitted about the middle of its glyph: a word starts
    // half a character before its first one, and ends with its last frame.
    let end = |cur: &mut Option<(String, f32, usize, usize, usize)>, at: &mut Vec<f32>, words: &mut Vec<Span>| {
        if let Some((text, sum, n, t0, t1)) = cur.take() {
            let half = if n > 1 { (t1 - t0) as f32 / (n - 1) as f32 / 2.0 } else { 1.0 };
            let u0 = (col(t0) - half * STEP as f32 * c.scale).max(0.0);
            words.push(Span { text, score: sum / n as f32, u0, u1: col(t1 + 1), at: std::mem::take(at) });
        }
    };
    for t in 0..t_len {
        let row = &flat[t * classes..(t + 1) * classes];
        let (k, &p) = row.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap();
        if k == space {
            end(&mut cur, &mut at, &mut words);
        } else if k != 0 && k < chars.len() {
            match &mut cur {
                Some((text, sum, n, _, t1)) => {
                    if k != prev {
                        text.push_str(&chars[k]);
                        *sum += p;
                        *n += 1;
                        at.push(col(t) + 0.5 * STEP as f32 * c.scale);
                    }
                    *t1 = t;
                }
                None => {
                    cur = Some((chars[k].clone(), p, 1, t, t));
                    at.push(col(t) + 0.5 * STEP as f32 * c.scale);
                }
            }
        }
        prev = k;
    }
    end(&mut cur, &mut at, &mut words);
    Ok(words)
}
