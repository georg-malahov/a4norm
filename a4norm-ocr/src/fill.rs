//! Where each answer goes and how large it is set (A4Norm Forms E3), from
//! the pages' `PageInspection`s and the form's template.
//!
//! A field of the template points at a candidate of its page by number, or
//! at a box the model drew (`box2d`, 0–1000 of the page, `[ymin, xmin,
//! ymax, xmax]`), which is snapped to the nearest candidate: a writing line
//! just under it, a field or a comb it lies in, a check box. A choice's
//! options are check boxes of their own, or else the boxes of the field's
//! row, left to right.
//!
//! One size for the document (the plan's amendment 3): `base = clamp(round(
//! printed + 2), 9, 0.72 x field)`, from the print's size and the typical
//! field's height. A value that does not fit is made smaller, only as far
//! as it has to and not below 7 pt; then it is broken over two lines if the
//! field is high enough; else it is set at 7 pt and marked as overflowing.
//! A comb takes a character per cell; a check box takes a cross. All in
//! points from the page's top left, in Helvetica.

use crate::helvetica::{self, CAP, DESCENT};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A page as `Ocr.inspect` describes it (only what placing needs).
#[derive(Debug, Clone, Deserialize)]
pub struct Inspection {
    /// 1-based; the page's place in the list when absent
    pub page: Option<u32>,
    #[serde(rename = "sizePt")]
    pub size_pt: [f32; 2],
    #[serde(default, rename = "skewDeg")]
    pub skew_deg: f32,
    #[serde(default)]
    pub words: Vec<IWord>,
    #[serde(default)]
    pub lines: Vec<ILine>,
    #[serde(default)]
    pub rects: Vec<IBox>,
    #[serde(default)]
    pub combs: Vec<IComb>,
    #[serde(default)]
    pub boxes: Vec<IBox>,
    #[serde(default, rename = "printedSize")]
    pub printed_size: f32,
    #[serde(default, rename = "typicalFieldHeight")]
    pub typical_field_height: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IWord {
    pub text: String,
    pub bbox: [f32; 4],
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ILine {
    pub id: usize,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct IBox {
    pub id: usize,
    #[serde(rename = "box")]
    pub b: [f32; 4],
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct IComb {
    pub id: usize,
    #[serde(rename = "box")]
    pub b: [f32; 4],
    pub cells: usize,
}

/// Where a field (or an option) is: a candidate's number on a page, or a
/// box, in 0–1000 of the page (`box2d`) or in points (`box`).
#[derive(Debug, Clone, Deserialize)]
pub struct Place {
    #[serde(default = "first")]
    pub page: u32,
    pub candidate: Option<usize>,
    #[serde(alias = "box_2d")]
    pub box2d: Option<[f32; 4]>,
    #[serde(rename = "box")]
    pub b: Option<[f32; 4]>,
}

fn first() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct Field {
    pub key: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    pub place: Option<Place>,
    #[serde(default)]
    pub options: Vec<Opt>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Opt {
    pub value: String,
    pub place: Option<Place>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Template {
    pub fields: Vec<Field>,
}

/// What `fill` is given besides the source pages.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub inspections: Vec<Inspection>,
    pub template: Template,
    /// a text, or for a choice the option's value, or `true` for a single box
    pub answers: HashMap<String, serde_json::Value>,
    pub color: Option<String>,
    #[serde(rename = "minSize")]
    pub min_size: Option<f32>,
}

/// An answer as placed: its print point (the first line's baseline at its
/// start; a cross's middle), size, lines, whether it overflows.
#[derive(Debug, Clone, Serialize)]
pub struct Placed {
    pub key: String,
    pub page: u32,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub lines: u8,
    pub overflow: bool,
    /// "text", "comb" or "check"
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<usize>,
    /// characters Helvetica cannot set were set as "?"
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub lost: bool,
}

/// What is drawn on a page, in points from its top left.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    /// text from its baseline's start, turned by `angle` radians (down to
    /// the right when positive, as the page's lines run)
    Text { page: u32, x: f32, y: f32, size: f32, angle: f32, text: String },
    /// a cross over `[x0, y0, x1, y1]`
    Cross { page: u32, b: [f32; 4], width: f32 },
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub base: f32,
    pub color: [f32; 3],
    pub placed: Vec<Placed>,
    pub marks: Vec<Mark>,
}

const LEADING: f32 = 1.15;
const PAD: f32 = 2.0; // from a field's side to the text

/// Where a value goes.
#[derive(Debug, Clone, Copy)]
enum Target {
    Line(ILine),
    Area([f32; 4]),
    Comb([f32; 4], usize),
    Check([f32; 4]),
}

/// The document's size: `clamp(round(printed + 2), 9, 0.72 x field)`, the
/// medians over its pages; below 9 pt when its fields are that low.
pub fn base_size(pages: &[Inspection]) -> f32 {
    let med = |mut v: Vec<f32>| {
        v.retain(|x| *x > 0.0);
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied()
    };
    let printed = med(pages.iter().map(|p| p.printed_size).collect()).unwrap_or(10.0);
    let field = med(pages.iter().map(|p| p.typical_field_height).collect()).unwrap_or(20.0);
    let cap = 0.72 * field;
    if cap < 9.0 {
        cap
    } else {
        (printed + 2.0).round().clamp(9.0, cap)
    }
}

pub fn layout(req: &Request) -> Layout {
    let base = base_size(&req.inspections);
    let min = req.min_size.unwrap_or(7.0).min(base);
    let color = req.color.as_deref().and_then(hex).unwrap_or([0x1a as f32 / 255.0; 3]);
    let mut out = Layout { base, color, placed: vec![], marks: vec![] };
    for f in &req.template.fields {
        let Some(answer) = req.answers.get(&f.key) else { continue };
        let Some(place) = &f.place else { continue };
        let Some(ins) = page(&req.inspections, place.page) else { continue };
        let choice = f.kind == "choice" || !f.options.is_empty();
        if choice {
            check(&mut out, f, place, ins, answer);
            continue;
        }
        let text = match answer {
            serde_json::Value::String(s) => s.trim().to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => continue,
        };
        if text.is_empty() {
            continue;
        }
        let Some((target, candidate)) = snap(ins, place, false) else { continue };
        let (placed, marks) = set(&text, target, ins, base, min, place.page);
        out.placed.push(Placed { key: f.key.clone(), candidate, ..placed });
        out.marks.extend(marks);
    }
    out
}

fn page(pages: &[Inspection], n: u32) -> Option<&Inspection> {
    pages.iter().enumerate().find(|(i, p)| p.page.unwrap_or(*i as u32 + 1) == n).map(|(_, p)| p)
}

fn hex(s: &str) -> Option<[f32; 3]> {
    let s = s.strip_prefix('#')?;
    let v = u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6)?;
    Some([(v >> 16) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0])
}

/// A choice: a cross in the chosen option's box, or in the field's own box
/// for a single box answered `true`.
fn check(out: &mut Layout, f: &Field, place: &Place, ins: &Inspection, answer: &serde_json::Value) {
    let chosen = match answer {
        serde_json::Value::Bool(true) => None,
        serde_json::Value::String(s) if !s.is_empty() => Some(s.as_str()),
        _ => return,
    };
    let target = match chosen {
        None => snap(ins, place, true),
        Some(v) => {
            let Some(i) = f.options.iter().position(|o| o.value == v) else { return };
            match &f.options[i].place {
                Some(p) => snap(ins, p, true),
                // the options' boxes are the row's, left to right
                None => row_boxes(ins, place).get(i).map(|b| (Target::Check(b.b), Some(b.id))),
            }
        }
    };
    let Some((Target::Check(b), candidate)) = target else { return };
    let side = (b[2] - b[0]).min(b[3] - b[1]);
    let inset = 0.18 * side;
    let cross = [b[0] + inset, b[1] + inset, b[2] - inset, b[3] - inset];
    out.marks.push(Mark::Cross { page: place.page, b: cross, width: (0.08 * side).clamp(0.6, 1.4) });
    out.placed.push(Placed {
        key: f.key.clone(),
        page: place.page,
        x: (b[0] + b[2]) / 2.0,
        y: (b[1] + b[3]) / 2.0,
        size: side,
        lines: 1,
        overflow: false,
        kind: "check",
        candidate,
        lost: false,
    });
}

/// The check boxes on the row of the field's place, left to right.
fn row_boxes(ins: &Inspection, place: &Place) -> Vec<IBox> {
    let Some(anchor) = area(ins, place) else { return vec![] };
    let mid = (anchor[1] + anchor[3]) / 2.0;
    let mut row: Vec<IBox> = ins.boxes.iter().copied().filter(|b| ((b.b[1] + b.b[3]) / 2.0 - mid).abs() <= 6.0).collect();
    row.sort_by(|a, b| a.b[0].total_cmp(&b.b[0]));
    row
}

/// The place's own box in points: the candidate's, or the model's.
fn area(ins: &Inspection, place: &Place) -> Option<[f32; 4]> {
    if let Some(id) = place.candidate {
        return candidate(ins, id).map(|(t, _)| match t {
            Target::Line(l) => [l.x0, l.y0.min(l.y1) - 20.0, l.x1, l.y0.max(l.y1)],
            Target::Area(b) | Target::Comb(b, _) | Target::Check(b) => b,
        });
    }
    place.b.or_else(|| {
        place.box2d.map(|[y0, x0, y1, x1]| {
            let [w, h] = ins.size_pt;
            [x0 / 1000.0 * w, y0 / 1000.0 * h, x1 / 1000.0 * w, y1 / 1000.0 * h]
        })
    })
}

fn candidate(ins: &Inspection, id: usize) -> Option<(Target, Option<usize>)> {
    let t = ins
        .lines
        .iter()
        .find(|l| l.id == id)
        .map(|l| Target::Line(*l))
        .or_else(|| ins.rects.iter().find(|r| r.id == id).map(|r| Target::Area(r.b)))
        .or_else(|| ins.combs.iter().find(|c| c.id == id).map(|c| Target::Comb(c.b, c.cells)))
        .or_else(|| ins.boxes.iter().find(|b| b.id == id).map(|b| Target::Check(b.b)))?;
    Some((t, Some(id)))
}

fn share(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    w * h / ((a[2] - a[0]) * (a[3] - a[1])).max(1e-3)
}

/// The candidate a place means. A number is taken as it is. A box snaps
/// to a check box it overlaps (for a choice), a field or comb holding half
/// of it, or a writing line under it (overlapping it by 14 pt, its foot
/// within 25 pt); else the box itself is the field.
fn snap(ins: &Inspection, place: &Place, want_check: bool) -> Option<(Target, Option<usize>)> {
    if let Some(id) = place.candidate {
        return candidate(ins, id);
    }
    let b = area(ins, place)?;
    if want_check {
        let best = ins.boxes.iter().map(|c| (share(&c.b, &b).max(share(&b, &c.b)), c)).max_by(|x, y| x.0.total_cmp(&y.0));
        return match best {
            Some((s, c)) if s > 0.2 => Some((Target::Check(c.b), Some(c.id))),
            _ => Some((Target::Check(b), None)),
        };
    }
    let held = |r: &[f32; 4]| share(&b, r);
    let comb = ins.combs.iter().map(|c| (held(&c.b), c)).max_by(|x, y| x.0.total_cmp(&y.0));
    let rect = ins.rects.iter().map(|r| (held(&r.b), r)).max_by(|x, y| x.0.total_cmp(&y.0));
    match (comb, rect) {
        (Some((s, c)), _) if s >= 0.5 => return Some((Target::Comb(c.b, c.cells), Some(c.id))),
        (_, Some((s, r))) if s >= 0.5 => return Some((Target::Area(r.b), Some(r.id))),
        _ => {}
    }
    let line = ins
        .lines
        .iter()
        .filter(|l| l.x1.min(b[2]) - l.x0.max(b[0]) >= 14.0)
        .map(|l| (((l.y0 + l.y1) / 2.0 - b[3]).abs(), l))
        .filter(|(d, _)| *d <= 25.0)
        .min_by(|x, y| x.0.total_cmp(&y.0));
    match line {
        Some((_, l)) => Some((Target::Line(*l), Some(l.id))),
        None => Some((Target::Area(b), None)),
    }
}

/// A text or a comb value set in its target.
fn set(text: &str, target: Target, ins: &Inspection, base: f32, min: f32, page: u32) -> (Placed, Vec<Mark>) {
    let lost = helvetica::encode(text).1;
    let placed = |x, y, size, lines, overflow, kind| Placed {
        key: String::new(),
        page,
        x,
        y,
        size,
        lines,
        overflow,
        kind,
        candidate: None,
        lost,
    };
    let skew = ins.skew_deg.to_radians();
    match target {
        Target::Comb(b, cells) => {
            let chars: Vec<String> = text.chars().filter(|c| !c.is_whitespace()).map(String::from).collect();
            let cw = (b[2] - b[0]) / cells.max(1) as f32;
            let widest = chars.iter().map(|c| helvetica::width(c, 1.0)).fold(0.5, f32::max);
            let size = base.min(0.7 * (b[3] - b[1])).min(0.8 * cw / widest);
            let y = (b[1] + b[3]) / 2.0 + CAP * size / 2.0;
            let marks = chars
                .iter()
                .take(cells)
                .enumerate()
                .map(|(i, c)| Mark::Text {
                    page,
                    x: b[0] + (i as f32 + 0.5) * cw - helvetica::width(c, size) / 2.0,
                    y,
                    size,
                    angle: skew,
                    text: c.clone(),
                })
                .collect();
            (placed(b[0] + 0.5 * cw, y, size, 1, chars.len() > cells, "comb"), marks)
        }
        Target::Line(l) => {
            let (x0, x1) = (l.x0 + PAD, l.x1 - 1.0);
            let at = |x: f32| l.y0 + (l.y1 - l.y0) * (x - l.x0) / (l.x1 - l.x0).max(1.0);
            let room = room_above(ins, &l);
            let (size, rows, overflow) = fit(text, x1 - x0, room, base, min);
            // descenders clear the line
            let foot = at(x0) - DESCENT * size - 0.8;
            let angle = ((l.y1 - l.y0) / (l.x1 - l.x0).max(1.0)).atan();
            let n = rows.len();
            let marks = rows
                .into_iter()
                .enumerate()
                .map(|(i, t)| Mark::Text { page, x: x0, y: foot - (n - 1 - i) as f32 * LEADING * size, size, angle, text: t })
                .collect::<Vec<_>>();
            let y = foot - (n - 1) as f32 * LEADING * size;
            (placed(x0, y, size, n as u8, overflow, "text"), marks)
        }
        Target::Area(b) | Target::Check(b) => {
            // a label printed in the field's top takes that part
            let label = ins
                .words
                .iter()
                .filter(|w| {
                    let (cx, cy) = ((w.bbox[0] + w.bbox[2]) / 2.0, (w.bbox[1] + w.bbox[3]) / 2.0);
                    cx > b[0] && cx < b[2] && cy > b[1] && cy < (b[1] + b[3]) / 2.0
                })
                .map(|w| w.bbox[3])
                .fold(b[1], f32::max);
            let z = [b[0] + PAD, label + 0.5, b[2] - PAD, b[3] - 0.5];
            let (size, rows, overflow) = fit(text, z[2] - z[0], z[3] - z[1], base, min);
            let n = rows.len() as f32;
            let first = (z[1] + z[3]) / 2.0 - (n - 1.0) * LEADING * size / 2.0 + CAP * size / 2.0;
            let marks = rows
                .into_iter()
                .enumerate()
                .map(|(i, t)| Mark::Text { page, x: z[0], y: first + i as f32 * LEADING * size, size, angle: skew, text: t })
                .collect();
            (placed(z[0], first, size, n as u8, overflow, "text"), marks)
        }
    }
}

/// The room above a writing line: down to it from the nearest word or line
/// above that overlaps it, at most three typical fields.
fn room_above(ins: &Inspection, l: &ILine) -> f32 {
    let y = l.y0.min(l.y1);
    let over = |x0: f32, x1: f32| x1.min(l.x1) - x0.max(l.x0) > 2.0;
    let above = ins
        .words
        .iter()
        .filter(|w| over(w.bbox[0], w.bbox[2]) && w.bbox[3] < y - 1.0)
        .map(|w| w.bbox[3])
        .chain(ins.lines.iter().filter(|o| o.id != l.id && over(o.x0, o.x1) && o.y0.max(o.y1) < y - 1.0).map(|o| o.y0.max(o.y1)))
        .fold(f32::MIN, f32::max);
    let most = 3.0 * ins.typical_field_height.max(10.0);
    if above == f32::MIN {
        most
    } else {
        (y - above).min(most)
    }
}

/// The size and lines of `text` in `w` x `h`: at `base` if it fits; else
/// smaller, down to `min`; else on two lines if they fit at `min` or more;
/// else one line at `min`, overflowing.
fn fit(text: &str, w: f32, h: f32, base: f32, min: f32) -> (f32, Vec<String>, bool) {
    let top = base.min(h / LEADING).max(min);
    let one = helvetica::width(text, top);
    if one <= w {
        return (top, vec![text.to_string()], false);
    }
    let s = (top * w / one * 10.0).floor() / 10.0;
    if s >= min {
        return (s, vec![text.to_string()], false);
    }
    // two lines, broken at the space that evens them
    let words: Vec<&str> = text.split(' ').collect();
    let best = (1..words.len())
        .map(|k| (words[..k].join(" "), words[k..].join(" ")))
        .min_by(|a, b| {
            let m = |p: &(String, String)| helvetica::width(&p.0, 1.0).max(helvetica::width(&p.1, 1.0));
            m(a).total_cmp(&m(b))
        });
    if let Some((a, b)) = best {
        let widest = helvetica::width(&a, 1.0).max(helvetica::width(&b, 1.0));
        let s2 = ((w / widest).min(top).min(h / (2.0 * LEADING)) * 10.0).floor() / 10.0;
        if s2 >= min {
            return (s2, vec![a, b], false);
        }
    }
    (min, vec![text.to_string()], true)
}

impl Layout {
    /// `{baseSize, placed: [...]}` for the site.
    pub fn json(&self) -> String {
        format!("{{\"baseSize\":{},\"placed\":{}}}", self.base, serde_json::to_string(&self.placed).unwrap())
    }
}
