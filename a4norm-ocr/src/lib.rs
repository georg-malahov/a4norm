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
/// The geometry of a form's page, shared with the scanner (a4norm-geometry)
pub use a4norm_geometry as geometry;
/// Filling a form in and writing the PDF, shared with the fill module
/// (a4norm-fill)
pub use a4norm_fill::{fill, font, pdf};
pub mod lang;
pub mod rec;

#[cfg(all(target_arch = "wasm32", feature = "wasm-threads"))]
pub mod pool;
#[cfg(target_arch = "wasm32")]
mod wasm;

use det::Frame;
use image::{imageops, RgbImage};
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
    /// where each character was read, across the page in its pixels
    pub at: Vec<f32>,
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
    /// how far the picture was turned clockwise to read it the right way
    /// up: 0, 90, 180 or 270. Everything else is on the page so turned;
    /// `width` and `height` are its.
    pub orientation: u16,
}

/// A line's box across the text is the body of its letters plus the room
/// the detector leaves around them, in points per point of type size.
const BOX_PER_PT: f32 = 1.39;

/// The lines read to tell which way up the page is: the longest few that
/// are short enough to read quickly (up to 480 of the recognizer's
/// columns, some 25 characters).
const SAMPLE: usize = 6;
const SAMPLE_WIDTH: usize = 480;
/// A sample read this confidently is the right way up; below it, it is
/// read upside down too, and the better reading wins.
const UPRIGHT: f32 = 0.85;

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
    /// and the languages, read the right way up (`Page::orientation`).
    pub fn page(&self, page: &RgbImage) -> TractResult<Page> {
        Ok(self.upright(page)?.0)
    }

    /// The page read the right way up, and the picture turned so when it
    /// had to be. When its lines run down the page (a sheet across the
    /// frame), a few are read both ways across and the page is turned the
    /// way they read better; a page whose lines run across but read better
    /// upside down is turned half round. Either costs a few short lines
    /// read and one more detection; a page the right way up costs nothing,
    /// the lines read to tell being kept.
    fn upright(&self, page: &RgbImage) -> TractResult<(Page, Option<RgbImage>)> {
        let (boxes, skew) = self.detect(page)?;
        if sideways(&boxes) {
            let f = Frame::new(skew);
            let tall: Vec<[f32; 4]> = boxes.iter().filter(|b| b.v1 - b.v0 > 3.0 * (b.u1 - b.u0)).map(|b| f.bbox(b.u0, b.v0, b.u1, b.v1)).collect();
            let sample = sample_of(&tall.iter().map(|b| (b[3] - b[1], b[2] - b[0])).collect::<Vec<_>>());
            let read = |turn: u16| -> TractResult<f32> {
                let got = map(&sample, |&i| {
                    let c = rec::cut_turned(page, tall[i], turn);
                    rec::read(&self.rec_plan(c.width)?, &c, &self.chars)
                });
                Ok(confidence(&got.into_iter().collect::<TractResult<Vec<_>>>()?))
            };
            let turn = if read(270)? > read(90)? { 270 } else { 90 };
            let t = if turn == 90 { imageops::rotate90(page) } else { imageops::rotate270(page) };
            let (b, s) = self.detect(&t)?;
            let p = self.read(&t, b, s, false)?.expect("read without the check");
            return Ok((Page { orientation: turn, ..p }, Some(t)));
        }
        if let Some(p) = self.read(page, boxes, skew, true)? {
            return Ok((p, None));
        }
        let t = imageops::rotate180(page);
        let (b, s) = self.detect(&t)?;
        let p = self.read(&t, b, s, false)?.expect("read without the check");
        Ok((Page { orientation: 180, ..p }, Some(t)))
    }

    fn detect(&self, page: &RgbImage) -> TractResult<(Vec<det::TextBox>, f32)> {
        let (pw, ph) = page.dimensions();
        let shape = det::shape(pw, ph, self.det_long);
        det::detect(&self.det_plan(shape)?, page, shape)
    }

    /// The lines in `boxes` read. With `check`, the longest are read first,
    /// and None if they read better upside down.
    fn read(&self, page: &RgbImage, mut boxes: Vec<det::TextBox>, skew: f32, check: bool) -> TractResult<Option<Page>> {
        let (pw, ph) = page.dimensions();
        let f = Frame::new(skew);
        boxes.retain(|b| b.u1 - b.u0 >= 4.0 && b.v1 - b.v0 >= 4.0);
        // reading order: by the line's middle across, in bands of a third of
        // a typical line, then along
        let band = median(boxes.iter().map(|b| b.v1 - b.v0).collect()).max(1.0) / 3.0;
        boxes.sort_by(|a, b| {
            let (ka, kb) = (((a.v0 + a.v1) / 2.0 / band) as i64, ((b.v0 + b.v1) / 2.0 / band) as i64);
            ka.cmp(&kb).then(a.u0.total_cmp(&b.u0))
        });

        let cuts: Vec<rec::Cut> = map(&boxes, |b| rec::cut(page, &f, b, false));
        let mut widths: Vec<usize> = cuts.iter().map(|c| c.width).collect();
        widths.sort_unstable();
        widths.dedup();
        let plans: Vec<(usize, Plan)> =
            map(&widths, |&w| self.rec_plan(w).map(|p| (w, p))).into_iter().collect::<TractResult<_>>()?;
        let plan = |w: usize| &plans.iter().find(|(pw, _)| *pw == w).unwrap().1;
        let mut read: Vec<Option<Vec<rec::Span>>> = (0..cuts.len()).map(|_| None).collect();
        if check {
            let longest = sample_of(&boxes.iter().map(|b| (b.u1 - b.u0, b.v1 - b.v0)).collect::<Vec<_>>());
            let sample = map(&longest, |&i| rec::read(plan(cuts[i].width), &cuts[i], &self.chars)).into_iter().collect::<TractResult<Vec<_>>>()?;
            let right = confidence(&sample);
            if right < UPRIGHT {
                let flipped = map(&longest, |&i| {
                    let c = rec::cut(page, &f, &boxes[i], true);
                    rec::read(plan(c.width), &c, &self.chars)
                })
                .into_iter()
                .collect::<TractResult<Vec<_>>>()?;
                if confidence(&flipped) > right {
                    return Ok(None);
                }
            }
            for (i, spans) in longest.into_iter().zip(sample) {
                read[i] = Some(spans);
            }
        }
        let rest: Vec<usize> = (0..cuts.len()).filter(|&i| read[i].is_none()).collect();
        let got = map(&rest, |&i| rec::read(plan(cuts[i].width), &cuts[i], &self.chars));
        for (i, spans) in rest.into_iter().zip(got) {
            read[i] = Some(spans?);
        }

        let mut lines = vec![];
        for ((b, spans), c) in boxes.iter().zip(read).zip(&cuts) {
            let end = c.tw as f32 * c.scale; // a word ends at most where the line does
            let words: Vec<Word> = spans
                .unwrap_or_default()
                .into_iter()
                .map(|s| Word {
                    bbox: f.bbox(b.u0 + s.u0, b.v0, b.u0 + s.u1.min(end), b.v1),
                    at: s.at.iter().map(|&u| f.page(b.u0 + u, (b.v0 + b.v1) / 2.0).0).collect(),
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
        Ok(Some(Page { width: pw, height: ph, skew, lines, printed, langs, orientation: 0 }))
    }
}

/// Whether the lines run down the page: tall boxes outweigh wide ones, by
/// their length, twice over.
fn sideways(boxes: &[det::TextBox]) -> bool {
    let (mut tall, mut wide) = (0.0, 0.0);
    for b in boxes {
        let (w, h) = (b.u1 - b.u0, b.v1 - b.v0);
        if h > 3.0 * w {
            tall += h;
        } else if w > 3.0 * h {
            wide += w;
        }
    }
    tall > 2.0 * wide
}

/// Which of the lines (`along` x `across` each) to read to tell which way
/// up the page is: the longest that read quickly, or the shortest few.
fn sample_of(lines: &[(f32, f32)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&a, &b| lines[b].0.total_cmp(&lines[a].0));
    let quick: Vec<usize> = order.iter().copied().filter(|&i| rec::width(lines[i].0, lines[i].1) <= SAMPLE_WIDTH).take(SAMPLE).collect();
    if quick.len() >= 3 {
        return quick;
    }
    order.into_iter().rev().take(3).collect()
}

/// The mean confidence of lines read, over their characters.
fn confidence(lines: &[Vec<rec::Span>]) -> f32 {
    let (mut sum, mut n) = (0.0, 0usize);
    for s in lines.iter().flatten() {
        let k = s.text.chars().count();
        sum += s.score * k as f32;
        n += k;
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f32
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

impl Ocr {
    /// The page read and its geometry found, for a page whose pixels span
    /// `size_pt`: A4Norm Forms' whole `PageInspection`, the right way up
    /// (`Page::orientation`). The words tell the geometry where letters are.
    pub fn inspect(&self, page: &RgbImage, size_pt: [f32; 2]) -> TractResult<(Page, geometry::Geometry)> {
        let (mut p, turned) = self.upright(page)?;
        let page = turned.as_ref().unwrap_or(page);
        let g = geometry::find(page, px_pt(page.width(), p.size_pt(size_pt)), &p.texts());
        p.split_at_boxes(&g);
        Ok((p, g))
    }
}

/// Pixels per point along the page's width, for a page of `size_pt`.
pub fn px_pt(width: u32, size_pt: [f32; 2]) -> f32 {
    width as f32 / size_pt[0]
}

/// `PageInspection` JSON: the words and the geometry of a page whose pixels
/// span `size_pt` (as given, before it was turned the right way up).
pub fn inspection_json(p: &Page, g: &geometry::Geometry, size_pt: [f32; 2]) -> String {
    let text = p.to_json(size_pt);
    format!("{},{}}}", &text[..text.len() - 1], g.json_fields(px_pt(p.width, p.size_pt(size_pt))))
}

impl Page {
    /// The size of the page the right way up, of a picture given as
    /// `size_pt`.
    pub fn size_pt(&self, size_pt: [f32; 2]) -> [f32; 2] {
        if self.orientation % 180 == 90 {
            [size_pt[1], size_pt[0]]
        } else {
            size_pt
        }
    }

    /// The words that are words, for the geometry: a letter or digit at
    /// least, read with some confidence ("□" read as a character is a box).
    pub fn texts(&self) -> Vec<geometry::Text> {
        self.lines
            .iter()
            .flat_map(|l| &l.words)
            .filter(|w| w.score >= 0.6 && w.text.chars().any(char::is_alphanumeric))
            .map(|w| geometry::Text {
                bbox: w.bbox,
                chars: if w.at.len() == w.text.chars().count() { w.text.chars().zip(w.at.iter().copied()).collect() } else { vec![] },
            })
            .collect()
    }

    /// Words read across check boxes, split at them: a row of boxes reads
    /// as one word ("zu:JaNein" over "zu: [] Ja [] Nein"), and each option's
    /// label is a word of its own. A character read where a box is, or its
    /// side read as "[" or "|" next to it, is dropped.
    pub fn split_at_boxes(&mut self, g: &geometry::Geometry) {
        let boxes: Vec<[f32; 4]> =
            g.candidates.iter().filter_map(|c| if let geometry::Kind::Box(b) = c.kind { Some(b) } else { None }).collect();
        // a box's side read as a character ("[", "|") where a word meets it
        let side = |c: char| "[]|(){}□口".contains(c);
        for line in &mut self.lines {
            let mut out = vec![];
            for w in line.words.drain(..) {
                let across: Vec<&[f32; 4]> = boxes
                    .iter()
                    .filter(|b| {
                        let over = w.bbox[3].min(b[3]) - w.bbox[1].max(b[1]);
                        over > 0.5 * (b[3] - b[1]) && b[0] > w.bbox[0] && b[2] < w.bbox[2]
                    })
                    .collect();
                if across.is_empty() || w.at.len() != w.text.chars().count() {
                    out.push(w);
                    continue;
                }
                // the edges a box leaves between the pieces
                let mut edges = vec![w.bbox[0]];
                let mut sorted = across.clone();
                sorted.sort_by(|a, b| a[0].total_cmp(&b[0]));
                for b in &sorted {
                    edges.extend([b[0], b[2]]);
                }
                edges.push(w.bbox[2]);
                for (k, piece) in edges.chunks(2).enumerate() {
                    let (x0, x1) = (piece[0], piece[1]);
                    let mut keep: Vec<(char, f32)> = w.text.chars().zip(w.at.iter().copied()).filter(|&(_, x)| x > x0 && x < x1).collect();
                    if k + 1 < edges.len() / 2 {
                        while keep.last().is_some_and(|p| side(p.0)) {
                            keep.pop();
                        }
                    }
                    if k > 0 {
                        while keep.first().is_some_and(|p| side(p.0)) {
                            keep.remove(0);
                        }
                    }
                    let text: String = keep.iter().map(|p| p.0).collect();
                    let at: Vec<f32> = keep.iter().map(|p| p.1).collect();
                    if !text.is_empty() {
                        out.push(Word { text, score: w.score, bbox: [x0, w.bbox[1], x1, w.bbox[3]], at });
                    }
                }
            }
            // a box's side read into the word beside it ("Ja[" before a box)
            for w in &mut out {
                let near = |x: f32| {
                    boxes.iter().any(|b| {
                        let over = w.bbox[3].min(b[3]) - w.bbox[1].max(b[1]) > 0.5 * (b[3] - b[1]);
                        let reach = 0.5 * (b[3] - b[1]);
                        over && ((x - b[0]).abs() < reach || (x - b[2]).abs() < reach)
                    })
                };
                while w.text.chars().count() > 1 && w.text.ends_with(side) && w.at.last().is_some_and(|&x| near(x)) {
                    w.text.pop();
                    w.at.pop();
                }
                while w.text.chars().count() > 1 && w.text.starts_with(side) && w.at.first().is_some_and(|&x| near(x)) {
                    w.text.remove(0);
                    w.at.remove(0);
                }
            }
            line.words = out;
        }
    }

    /// A4Norm Forms' `PageInspection` for this page, without the geometry:
    /// `sizePt`, `orientation`, `skewDeg`, `words` (boxes in points from the
    /// top left), `printedSize` in points, `langs`, all of the page the
    /// right way up. The picture as given spans `size_pt`.
    pub fn to_json(&self, size_pt: [f32; 2]) -> String {
        let size_pt = self.size_pt(size_pt);
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
            "{{\"sizePt\":[{:.2},{:.2}],\"orientation\":{},\"skewDeg\":{:.3},\"words\":[{}],\"printedSize\":{:.1},\"langs\":[{}]}}",
            size_pt[0],
            size_pt[1],
            self.orientation,
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
