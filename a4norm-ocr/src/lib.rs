//! Text on a page and where it sits: PP-OCRv5 mobile detection and the latin
//! recognizer, run by tract (pure Rust, so the same code builds for the
//! browser). The models are not bundled: the caller hands their bytes in.
//!
//! One detection finds the text lines and the page's skew; each line is cut
//! out along its tilted box and read, in parallel with the `par` feature.
//! The result is a page's words with their boxes, its skew, the size of its
//! print and its languages: A4Norm Forms' `PageInspection` without the
//! geometry.

pub mod det;
pub mod lang;
pub mod rec;

#[cfg(all(target_arch = "wasm32", feature = "wasm-threads"))]
pub mod pool;
#[cfg(target_arch = "wasm32")]
mod wasm;

use det::Frame;
use image::RgbImage;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use tract_onnx::prelude::*;

pub(crate) type Plan = Arc<TypedRunnableModel>;

/// A word: its text, the recognizer's mean confidence over its characters,
/// and its upright box `[x0, y0, x1, y1]` in page pixels.
#[derive(Debug, Clone)]
pub struct Word {
    pub text: String,
    pub score: f32,
    pub bbox: [f32; 4],
}

/// A text line as detected: its words left to right, its upright box, and
/// its height across the line in page pixels.
#[derive(Debug, Clone)]
pub struct Line {
    pub words: Vec<Word>,
    pub bbox: [f32; 4],
    pub height: f32,
}

impl Line {
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

/// What the page holds, in its pixels.
#[derive(Debug, Clone)]
pub struct Page {
    pub width: u32,
    pub height: u32,
    /// radians; the lines run down to the right when positive
    pub skew: f32,
    /// top to bottom, then left to right
    pub lines: Vec<Line>,
    /// the size of the print, median over characters, in page pixels
    pub printed: f32,
    pub langs: Vec<&'static str>,
}

/// A line's box across the text is the body of its letters plus the room
/// the detector leaves around them, in points per point of type size.
const BOX_PER_PT: f32 = 1.39;

pub struct Ocr {
    det: InferenceModel,
    rec: InferenceModel,
    /// Index 0 is the CTC blank; then the dictionary; then a space.
    chars: Vec<String>,
    det_plans: Mutex<Vec<((usize, usize), Plan)>>,
    rec_plans: Mutex<Vec<(usize, Plan)>>,
    /// the long side of the page as detection sees it
    pub det_long: u32,
}

impl Ocr {
    /// The detector's and the recognizer's `inference.onnx`, and the
    /// recognizer's `inference.yml`, which holds its character list.
    pub fn new(det_onnx: &[u8], rec_onnx: &[u8], rec_yml: &str) -> TractResult<Ocr> {
        // The exported graphs carry symbolic shapes that tract cannot unify
        // with a concrete input: their declared facts are dropped.
        let load = |b: &[u8]| {
            tract_onnx::onnx()
                .with_ignore_value_info(true)
                .with_ignore_output_shapes(true)
                .model_for_read(&mut Cursor::new(b))
        };
        let dict = character_dict(rec_yml);
        if dict.is_empty() {
            return Err(TractError::msg("no PostProcess.character_dict in the recognizer's inference.yml"));
        }
        let mut chars = vec![String::new()];
        chars.extend(dict);
        chars.push(" ".to_string());
        Ok(Ocr {
            det: load(det_onnx)?,
            rec: load(rec_onnx)?,
            chars,
            det_plans: Mutex::new(vec![]),
            rec_plans: Mutex::new(vec![]),
            det_long: 960,
        })
    }

    fn det_plan(&self, shape: (usize, usize)) -> TractResult<Plan> {
        if let Some((_, p)) = self.det_plans.lock().unwrap().iter().find(|(s, _)| *s == shape) {
            return Ok(p.clone());
        }
        let plan = compile(&self.det, [1, 3, shape.0, shape.1])?;
        self.det_plans.lock().unwrap().push((shape, plan.clone()));
        Ok(plan)
    }

    fn rec_plan(&self, width: usize) -> TractResult<Plan> {
        if let Some((_, p)) = self.rec_plans.lock().unwrap().iter().find(|(w, _)| *w == width) {
            return Ok(p.clone());
        }
        let plan = compile(&self.rec, [1, 3, rec::H, width])?;
        self.rec_plans.lock().unwrap().push((width, plan.clone()));
        Ok(plan)
    }

    /// Every text line of the page and its words, the skew, the print size
    /// and the languages.
    pub fn page(&self, page: &RgbImage) -> TractResult<Page> {
        let (pw, ph) = page.dimensions();
        let shape = det::shape(pw, ph, self.det_long);
        let (mut boxes, skew) = det::detect(&self.det_plan(shape)?, page, shape)?;
        let f = Frame::new(skew);
        boxes.retain(|b| b.u1 - b.u0 >= 4.0 && b.v1 - b.v0 >= 4.0);
        // reading order: by the line's middle across, in bands of a third of
        // a typical line, then along
        let band = median(boxes.iter().map(|b| b.v1 - b.v0).collect()).max(1.0) / 3.0;
        boxes.sort_by(|a, b| {
            let (ka, kb) = (((a.v0 + a.v1) / 2.0 / band) as i64, ((b.v0 + b.v1) / 2.0 / band) as i64);
            ka.cmp(&kb).then(a.u0.total_cmp(&b.u0))
        });

        let cuts: Vec<rec::Cut> = map(&boxes, |b| rec::cut(page, &f, b));
        let mut widths: Vec<usize> = cuts.iter().map(|c| c.width).collect();
        widths.sort_unstable();
        widths.dedup();
        let plans: Vec<(usize, Plan)> =
            map(&widths, |&w| self.rec_plan(w).map(|p| (w, p))).into_iter().collect::<TractResult<_>>()?;
        let read = map(&cuts, |c| {
            let plan = &plans.iter().find(|(w, _)| *w == c.width).unwrap().1;
            rec::read(plan, c, &self.chars)
        });

        let mut lines = vec![];
        for ((b, spans), c) in boxes.iter().zip(read).zip(&cuts) {
            let end = c.tw as f32 * c.scale; // a word ends at most where the line does
            let words: Vec<Word> = spans?
                .into_iter()
                .map(|s| Word {
                    bbox: f.bbox(b.u0 + s.u0, b.v0, b.u0 + s.u1.min(end), b.v1),
                    text: s.text,
                    score: s.score,
                })
                .collect();
            if !words.is_empty() {
                lines.push(Line { words, bbox: f.bbox(b.u0, b.v0, b.u1, b.v1), height: b.v1 - b.v0 });
            }
        }
        let printed = printed_size(&lines);
        let words = lines.iter().flat_map(|l| &l.words).filter(|w| w.score >= 0.7);
        let langs = lang::langs(words.map(|w| w.text.as_str()));
        Ok(Page { width: pw, height: ph, skew, lines, printed, langs })
    }
}

/// The recognizer's characters: the `character_dict` list of its
/// inference.yml, one `- c` per line, plain or in single quotes.
pub fn character_dict(yml: &str) -> Vec<String> {
    let mut lines = yml.lines().skip_while(|l| l.trim() != "character_dict:").skip(1);
    let mut out = vec![];
    while let Some(c) = lines.next().and_then(|l| l.trim_start().strip_prefix("- ")) {
        out.push(match c.strip_prefix('\'').and_then(|c| c.strip_suffix('\'')) {
            Some(q) => q.replace("''", "'"),
            None => c.to_string(),
        });
    }
    out
}

fn compile<const N: usize>(model: &InferenceModel, shape: [usize; N]) -> TractResult<Plan> {
    Ok(model.clone().with_input_fact(0, f32::fact(shape).into())?.into_optimized()?.into_runnable()?)
}

/// `f` over `items`, on rayon's threads with the `par` feature.
fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync + Send) -> Vec<R> {
    #[cfg(feature = "par")]
    {
        use rayon::prelude::*;
        items.par_iter().map(f).collect()
    }
    #[cfg(not(feature = "par"))]
    items.iter().map(f).collect()
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// The size of the print in page pixels: each confidently read line's box
/// height over BOX_PER_PT, the median over characters so that the body text
/// outweighs a few big headings.
fn printed_size(lines: &[Line]) -> f32 {
    let mut sizes: Vec<(f32, usize)> = lines
        .iter()
        .filter(|l| l.words.iter().all(|w| w.score >= 0.8))
        .map(|l| (l.height / BOX_PER_PT, l.words.iter().map(|w| w.text.chars().count()).sum()))
        .collect();
    sizes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: usize = sizes.iter().map(|s| s.1).sum();
    let mut acc = 0;
    for (s, n) in &sizes {
        acc += n;
        if 2 * acc >= total {
            return *s;
        }
    }
    0.0
}

impl Page {
    /// A4Norm Forms' `PageInspection` for this page, without the geometry:
    /// `sizePt`, `skewDeg`, `words` (boxes in points from the top left),
    /// `printedSize` in points, `langs`. The page's pixels span `size_pt`.
    pub fn to_json(&self, size_pt: [f32; 2]) -> String {
        let (sx, sy) = (size_pt[0] / self.width as f32, size_pt[1] / self.height as f32);
        let pt = |b: &[f32; 4]| format!("[{:.2},{:.2},{:.2},{:.2}]", b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy);
        let words: Vec<String> = self
            .lines
            .iter()
            .flat_map(|l| &l.words)
            .map(|w| format!("{{\"text\":{},\"bbox\":{},\"score\":{:.3}}}", json_str(&w.text), pt(&w.bbox), w.score))
            .collect();
        let langs: Vec<String> = self.langs.iter().map(|l| format!("\"{l}\"")).collect();
        format!(
            "{{\"sizePt\":[{:.2},{:.2}],\"skewDeg\":{:.3},\"words\":[{}],\"printedSize\":{:.1},\"langs\":[{}]}}",
            size_pt[0],
            size_pt[1],
            self.skew.to_degrees(),
            words.join(","),
            self.printed * sy,
            langs.join(",")
        )
    }
}

pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// The first JPEG in a PDF, as a scanner writes a page: the bytes between
/// `stream` and `endstream` of the first `/DCTDecode` object.
pub fn pdf_jpeg(pdf: &[u8]) -> Option<&[u8]> {
    let find = |hay: &[u8], needle: &[u8], from: usize| {
        hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|i| i + from)
    };
    let dct = find(pdf, b"/DCTDecode", 0)?;
    let s = find(pdf, b"stream", dct)? + 6;
    let s = s + if pdf.get(s) == Some(&b'\r') { 2 } else { 1 };
    let e = find(pdf, b"\xff\xd9", s)? + 2; // the JPEG's own end
    Some(&pdf[s..e])
}
