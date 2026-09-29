//! Where each answer goes and how large it is set (A4Norm Forms E3), from
//! the pages' `PageInspection`s and the form's template.
//!
//! A field of the template points at a candidate of its page by number (or
//! at several: a tax ID in four combs, an address over two lines), or at a
//! box the model drew (`box2d`, 0–1000 of the page, `[ymin, xmin,
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
//! points from the page's top left, in Arimo (font.rs).
//!
//! By hand, on the site: a field's `size` sets its value at that size
//! exactly (broken over two lines as usual, and left out of the page's
//! shared size), and its `shift` moves all it puts on the page, text,
//! cells and cross alike. The request's `texts` are free text of the
//! person's own, set where they put it.

use crate::font::{self, CAP, DESCENT};
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

/// Where a field (or an option) is: a candidate's number on a page, or
/// several in order, or a box, in 0–1000 of the page (`box2d`) or in
/// points (`box`).
#[derive(Debug, Clone, Deserialize)]
pub struct Place {
    #[serde(default = "first")]
    pub page: u32,
    pub candidate: Option<usize>,
    /// a value that runs on: through the cells of several combs, or from
    /// one line or field into the next
    #[serde(default)]
    pub candidates: Vec<usize>,
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
    /// the value's size in points, exactly, instead of the one chosen
    pub size: Option<f32>,
    /// `[dx, dy]` in points, right and down: moves all the field puts on
    /// its page
    pub shift: Option<[f32; 2]>,
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
    /// pictures to set over the pages (a signature, a stamp): their bytes
    /// come apart, in this order
    #[serde(default)]
    pub images: Vec<ImagePlace>,
    /// free text to set over the pages
    #[serde(default)]
    pub texts: Vec<FreeText>,
}

/// Free text: from its baseline's start `(x, y)`, in points from the
/// page's top left, at `size`, level.
#[derive(Debug, Clone, Deserialize)]
pub struct FreeText {
    #[serde(default = "first")]
    pub page: u32,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub text: String,
}

/// A picture's place: fitted into `box` (points from the page's top left),
/// its shape kept, in the middle.
#[derive(Debug, Clone, Deserialize)]
pub struct ImagePlace {
    #[serde(default = "first")]
    pub page: u32,
    #[serde(rename = "box")]
    pub b: [f32; 4],
    pub key: Option<String>,
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
    /// characters the font lacks were set as "?"
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub lost: bool,
    /// the height the text had, to tell a notably higher field
    #[serde(skip)]
    pub room: f32,
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

/// The sizes a value may take: at most `cap`, from the ladder `base`,
/// `base - 1`, … down to `min`, so that fields made smaller share sizes.
/// An `exact` size is `base`, whatever room there is.
#[derive(Debug, Clone, Copy)]
struct Sizes {
    base: f32,
    cap: f32,
    min: f32,
    exact: bool,
}

impl Sizes {
    /// The ladder's step at or below `s`.
    fn step(&self, s: f32) -> f32 {
        if s >= self.base {
            self.base
        } else {
            (self.base - (self.base - s - 1e-3).ceil()).max(self.min)
        }
    }
}
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

impl Request {
    /// The `sizePt` of page `n` (from 1), if it was inspected.
    pub fn page_size(&self, n: u32) -> Option<[f32; 2]> {
        page(&self.inspections, n).map(|p| p.size_pt)
    }
}

pub fn layout(req: &Request) -> Layout {
    let base = base_size(&req.inspections);
    let min = req.min_size.unwrap_or(7.0).min(base);
    let color = req.color.as_deref().and_then(hex).unwrap_or([0x1a as f32 / 255.0; 3]);
    let mut out = Layout { base, color, placed: vec![], marks: vec![] };
    let full = Sizes { base, cap: base, min, exact: false };
    // the text values, each at the largest size it takes
    let mut texts: Vec<(&Field, String, Placed, Vec<Mark>)> = vec![];
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
        let z = match f.size {
            Some(s) if s > 0.0 => Sizes { base: s, cap: s, min: s, exact: true },
            _ => full,
        };
        if let Some((p, m)) = place_text(&text, place, ins, z) {
            texts.push((f, text, p, m));
        }
    }
    // One size for most of a page: when more than half its text values had
    // to be made smaller, the others come down to the step most of those
    // took, unless their field is notably higher (half again the usual).
    // A size set by hand stays as it is, and counts for none of this.
    let pages: std::collections::BTreeSet<u32> = texts.iter().map(|t| t.2.page).collect();
    for pg in pages {
        let on: Vec<usize> = (0..texts.len())
            .filter(|&i| texts[i].2.page == pg && texts[i].2.kind == "text" && texts[i].0.size.is_none())
            .collect();
        let smaller: Vec<f32> = on.iter().map(|&i| texts[i].2.size).filter(|&s| s < base).collect();
        if on.len() < 2 || smaller.len() * 2 <= on.len() {
            continue;
        }
        let mut counts: Vec<(f32, usize)> = vec![];
        for s in &smaller {
            match counts.iter_mut().find(|c| (c.0 - s).abs() < 1e-3) {
                Some(c) => c.1 += 1,
                None => counts.push((*s, 1)),
            }
        }
        let step = counts.iter().max_by(|a, b| a.1.cmp(&b.1).then(a.0.total_cmp(&b.0))).unwrap().0;
        let mut rooms: Vec<f32> = on.iter().map(|&i| texts[i].2.room).collect();
        rooms.sort_by(f32::total_cmp);
        let usual = rooms[rooms.len() / 2];
        for &i in &on {
            let (f, text, p, _) = &texts[i];
            if p.size > step && p.room < 1.5 * usual {
                let place = f.place.as_ref().unwrap();
                let ins = page(&req.inspections, place.page).unwrap();
                if let Some(again) = place_text(text, place, ins, Sizes { cap: step, ..full }) {
                    (texts[i].2, texts[i].3) = again;
                }
            }
        }
    }
    for (f, _, mut p, mut m) in texts {
        if let Some(d) = f.shift {
            shift(&mut p, &mut m, d);
        }
        out.placed.push(Placed { key: f.key.clone(), ..p });
        out.marks.extend(m);
    }
    for t in &req.texts {
        let text = t.text.trim_end();
        if !text.is_empty() && t.size > 0.0 {
            out.marks.push(Mark::Text { page: t.page, x: t.x, y: t.y, size: t.size, angle: 0.0, text: text.to_string() });
        }
    }
    // in the template's order
    let order = |k: &str| req.template.fields.iter().position(|f| f.key == k);
    out.placed.sort_by_key(|p| order(&p.key));
    out
}

/// A text value at its place: one candidate, or several it runs on over.
fn place_text(text: &str, place: &Place, ins: &Inspection, z: Sizes) -> Option<(Placed, Vec<Mark>)> {
    if place.candidates.len() > 1 {
        return run_on(text, &place.candidates, ins, z, place.page);
    }
    let one = Place { candidate: place.candidate.or(place.candidates.first().copied()), ..place.clone() };
    let (target, candidate) = snap(ins, &one, false)?;
    let (placed, marks) = set(text, target, ins, z, place.page);
    Some((Placed { candidate, ..placed }, marks))
}

/// A value and its marks moved by `[dx, dy]`.
fn shift(p: &mut Placed, marks: &mut [Mark], [dx, dy]: [f32; 2]) {
    (p.x, p.y) = (p.x + dx, p.y + dy);
    for m in marks {
        match m {
            Mark::Text { x, y, .. } => (*x, *y) = (*x + dx, *y + dy),
            Mark::Cross { b, .. } => *b = [b[0] + dx, b[1] + dy, b[2] + dx, b[3] + dy],
        }
    }
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
    let mut marks = [Mark::Cross { page: place.page, b: cross, width: (0.08 * side).clamp(0.6, 1.4) }];
    let mut placed = Placed {
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
        room: side,
    };
    if let Some(d) = f.shift {
        shift(&mut placed, &mut marks, d);
    }
    out.marks.extend(marks);
    out.placed.push(placed);
}

/// A value over several candidates, in the order of their numbers: through
/// the cells of combs one after another; or into lines and fields, as many
/// words into each as fit at the base size, the rest into the next, the
/// last taking what is left by the usual rules. Placed where it starts.
fn run_on(text: &str, ids: &[usize], ins: &Inspection, z: Sizes, page: u32) -> Option<(Placed, Vec<Mark>)> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    let targets: Vec<Target> = ids.iter().filter_map(|&id| candidate(ins, id).map(|t| t.0)).collect();
    if targets.is_empty() {
        return None;
    }
    let mut chunks: Vec<String> = vec![];
    if targets.iter().all(|t| matches!(t, Target::Comb(..))) {
        let mut chars = text.chars().filter(|c| !c.is_whitespace());
        for (i, t) in targets.iter().enumerate() {
            let Target::Comb(_, n) = t else { unreachable!() };
            // the last comb takes the rest, and overflows with it
            let take = if i + 1 == targets.len() { usize::MAX } else { *n };
            chunks.push(chars.by_ref().take(take).collect());
        }
    } else {
        let mut words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
        for (i, t) in targets.iter().enumerate() {
            if i + 1 == targets.len() {
                chunks.push(words.join(" "));
                break;
            }
            let w = zone_width(t);
            let mut k = 0;
            while k < words.len() && font::width(&words[..=k].join(" "), z.cap) <= w {
                k += 1;
            }
            let k = k.max(1).min(words.len());
            chunks.push(words[..k].join(" "));
            words.drain(..k);
        }
    }
    let mut first: Option<Placed> = None;
    let (mut marks, mut lines, mut overflow) = (vec![], 0u8, false);
    for ((t, chunk), id) in targets.iter().zip(&chunks).zip(&ids) {
        if chunk.is_empty() {
            continue;
        }
        let (p, m) = set(chunk, *t, ins, z, page);
        (lines, overflow) = (lines + p.lines, overflow | p.overflow);
        first.get_or_insert(Placed { candidate: Some(*id), ..p });
        marks.extend(m);
    }
    let mut placed = first?;
    (placed.lines, placed.overflow, placed.lost) = (lines, overflow, font::lost(text));
    Some((placed, marks))
}

/// The width a target leaves the text.
fn zone_width(t: &Target) -> f32 {
    match t {
        Target::Line(l) => l.x1 - 1.0 - (l.x0 + PAD),
        Target::Area(b) | Target::Check(b) | Target::Comb(b, _) => b[2] - b[0] - 2.0 * PAD,
    }
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
fn set(text: &str, target: Target, ins: &Inspection, z: Sizes, page: u32) -> (Placed, Vec<Mark>) {
    let lost = font::lost(text);
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
        room: 0.0,
    };
    let skew = ins.skew_deg.to_radians();
    match target {
        Target::Comb(b, cells) => {
            let chars: Vec<String> = text.chars().filter(|c| !c.is_whitespace()).map(String::from).collect();
            let cw = (b[2] - b[0]) / cells.max(1) as f32;
            let widest = chars.iter().map(|c| font::width(c, 1.0)).fold(0.5, f32::max);
            let size = if z.exact { z.base } else { z.base.min(0.7 * (b[3] - b[1])).min(0.8 * cw / widest) };
            let y = (b[1] + b[3]) / 2.0 + CAP * size / 2.0;
            let marks = chars
                .iter()
                .take(cells)
                .enumerate()
                .map(|(i, c)| Mark::Text {
                    page,
                    x: b[0] + (i as f32 + 0.5) * cw - font::width(c, size) / 2.0,
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
            let (size, rows, overflow) = fit(text, x1 - x0, room, z);
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
            (Placed { room, ..placed(x0, y, size, n as u8, overflow, "text") }, marks)
        }
        Target::Area(b) | Target::Check(b) => {
            // the text goes below every printed word inside the frame (its
            // label, "Familienname" at the top of KG 1's fields)
            let label = ins
                .words
                .iter()
                .filter(|w| {
                    let (cx, cy) = ((w.bbox[0] + w.bbox[2]) / 2.0, (w.bbox[1] + w.bbox[3]) / 2.0);
                    cx > b[0] && cx < b[2] && cy > b[1] && cy < b[3]
                })
                .map(|w| w.bbox[3])
                .fold(b[1], f32::max)
                .min(b[3] - 1.0);
            let zone = [b[0] + PAD, label + 0.5, b[2] - PAD, b[3] - 0.5];
            let (size, rows, overflow) = fit(text, zone[2] - zone[0], zone[3] - zone[1], z);
            // the lines' glyphs, cap to descender, centred in the zone
            let n = rows.len() as f32;
            let first = (zone[1] + zone[3]) / 2.0 - (n - 1.0) * LEADING * size / 2.0 + (CAP - DESCENT) * size / 2.0;
            let marks = rows
                .into_iter()
                .enumerate()
                .map(|(i, t)| Mark::Text { page, x: zone[0], y: first + i as f32 * LEADING * size, size, angle: skew, text: t })
                .collect();
            (Placed { room: zone[3] - zone[1], ..placed(zone[0], first, size, n as u8, overflow, "text") }, marks)
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

/// The size and lines of `text` in `w` x `h`: as large as it may be, if it
/// fits; else smaller, down to `min`; else on two lines if they fit at
/// `min` or more; else one line at `min`, overflowing. Sizes below `base`
/// are steps of its ladder.
fn fit(text: &str, w: f32, h: f32, z: Sizes) -> (f32, Vec<String>, bool) {
    // a size set by hand takes the height it needs
    let h = if z.exact { f32::INFINITY } else { h };
    // one line needs its glyphs' height, cap to descender, and a little air
    let top = z.step(z.cap.min(h / (CAP + DESCENT + 0.05)).max(z.min));
    let one = font::width(text, top);
    if one <= w {
        return (top, vec![text.to_string()], false);
    }
    let s = top * w / one;
    if s >= z.min {
        return (z.step(s), vec![text.to_string()], false);
    }
    // two lines, broken at the space that evens them
    let words: Vec<&str> = text.split(' ').collect();
    let best = (1..words.len())
        .map(|k| (words[..k].join(" "), words[k..].join(" ")))
        .min_by(|a, b| {
            let m = |p: &(String, String)| font::width(&p.0, 1.0).max(font::width(&p.1, 1.0));
            m(a).total_cmp(&m(b))
        });
    if let Some((a, b)) = best {
        let widest = font::width(&a, 1.0).max(font::width(&b, 1.0));
        let s2 = (w / widest).min(top).min(h / (2.0 * LEADING));
        if s2 >= z.min {
            return (z.step(s2), vec![a, b], false);
        }
    }
    (z.min, vec![text.to_string()], true)
}

impl Layout {
    /// `{baseSize, placed: [...]}` for the site.
    pub fn json(&self) -> String {
        format!("{{\"baseSize\":{},\"placed\":{}}}", self.base, serde_json::to_string(&self.placed).unwrap())
    }
}
