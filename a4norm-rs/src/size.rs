//! A document's real size, where it can be told, and where it goes on the
//! page.
//!
//! A photo holds no physical size: the focal length and the sensor give an
//! angle per pixel, not the distance. So the size comes from what the
//! document says of itself:
//! - its machine-readable zone (mrz.rs), a ruler at 2.54 mm a character:
//!   a passport (TD3) page or spread, a TD2 card or visa;
//! - an ID-1 card's rounded shape (the cards path, laid out apart);
//! - else only its proportions, which give candidates, never a decision:
//!   A4, A5, A6, a passport's page and ID-2 all stand at 1.41-1.42.
//!
//! Only the zone and the card are laid at real size by themselves. The
//! person may place the document by hand (`Place`): at a width in mm, at
//! the size found or suggested, or over the page as before.

use crate::mrz::Mrz;

/// Standard formats, long side by short side, in mm.
pub const STANDARD: [(&str, f64, f64); 9] = [
    ("id1", 85.6, 53.98),
    ("td2", 105.0, 74.0),
    ("id3", 125.0, 88.0),
    ("id3-spread", 176.0, 125.0),
    ("a6", 148.0, 105.0),
    ("a5", 210.0, 148.0),
    ("a4", 297.0, 210.0),
    ("letter", 279.4, 215.9),
    ("dl", 220.0, 110.0),
];

/// Always among the candidates, for the person to choose from.
const ALWAYS: [&str; 4] = ["id1", "id3", "a5", "a6"];

/// A size the document may have: `mm` as the document lies (width, height).
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub kind: &'static str,
    pub mm: [f64; 2],
    pub confidence: f64,
}

/// What is known of the document's size.
#[derive(Debug, Clone, PartialEq)]
pub struct Size {
    pub kind: Option<&'static str>,
    pub mm: Option<[f64; 2]>,
    pub confidence: f64,
    /// "mrz", "card", "aspect" or "none"
    pub by: &'static str,
    /// laid on the page at that size
    pub applied: bool,
    pub candidates: Vec<Candidate>,
}

/// Where the person wants the document: at its real size (the size found,
/// or the best candidate), over the page, or `w` mm wide with its top left
/// at `(x, y)` mm (centred, 15 mm from the top, when not given).
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Real,
    Fit,
    At { w: f64, x: Option<f64>, y: Option<f64> },
}

/// The page's top margin for a document at a set size.
pub const TOP_MM: f64 = 15.0;

/// The standard formats a `w` x `h` document may be, by its proportions
/// alone, best first; those in ALWAYS whatever their fit (confidence 0).
pub fn by_aspect(w: f64, h: f64) -> Vec<Candidate> {
    let r = w.max(h) / w.min(h).max(1e-6);
    let wide = w >= h;
    let mut out: Vec<Candidate> = STANDARD
        .iter()
        .filter_map(|&(kind, l, s)| {
            let fit = 1.0 - (r / (l / s) - 1.0).abs() / 0.04;
            let confidence = (0.5 * fit).max(0.0);
            (confidence > 0.0 || ALWAYS.contains(&kind)).then(|| Candidate { kind, mm: if wide { [l, s] } else { [s, l] }, confidence: round2(confidence) })
        })
        .collect();
    out.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    out
}

/// The size a zone tells: the document's mm at the zone's scale, named
/// when within 8 % of a standard of its kind (a spread's pages bow).
pub fn by_mrz(m: &Mrz, w_px: f64, h_px: f64, at_bottom: bool) -> Size {
    let k = m.mm_per_px();
    let (w, h) = (w_px * k, h_px * k);
    let named: &[&str] = match m.chars {
        44 => &["id3", "id3-spread"],
        36 => &["td2"],
        _ => &["id1"],
    };
    let near = STANDARD.iter().filter(|s| named.contains(&s.0)).find(|&&(_, l, s)| {
        let (dl, ds) = if w >= h { (w / l, h / s) } else { (h / l, w / s) };
        (dl - 1.0).abs() < 0.08 && (ds - 1.0).abs() < 0.08
    });
    // the zone whole, or one line of it at the document's foot, where a
    // zone is; one line elsewhere is weaker
    let confidence = if m.lines >= 2 { 0.95 } else if at_bottom { 0.9 } else { 0.75 };
    let mut candidates = by_aspect(w, h);
    let kind = near.map(|s| s.0);
    if let Some(kind) = kind {
        candidates.retain(|c| c.kind != kind);
    }
    candidates.insert(0, Candidate { kind: kind.unwrap_or(m.kind_name()), mm: [round1(w), round1(h)], confidence });
    Size { kind, mm: Some([round1(w), round1(h)]), confidence, by: "mrz", applied: false, candidates }
}

/// Nothing but the proportions.
pub fn by_shape(w: f64, h: f64) -> Size {
    let candidates = by_aspect(w, h);
    Size { kind: None, mm: None, confidence: 0.0, by: if w > 0.0 { "aspect" } else { "none" }, applied: false, candidates }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

impl Mrz {
    fn kind_name(&self) -> &'static str {
        match self.chars {
            44 => "id3",
            36 => "td2",
            _ => "id1",
        }
    }
}

/// A page's document: what is known of its size, where it lies on the
/// page (mm from the top left: x, y, w, h), its pixels after rectifying
/// and trimming, and the page (mm).
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub size: Size,
    pub placed: [f64; 4],
    pub content: (usize, usize),
    pub sheet: (f64, f64),
}

impl Layout {
    /// `"size":…,"placed":…,"content":…,"sheet":…` for --json and the page.
    pub fn json_fields(&self) -> String {
        let s = &self.size;
        let mm = |m: [f64; 2]| format!("[{},{}]", m[0], m[1]);
        let cands: Vec<String> =
            s.candidates.iter().map(|c| format!("{{\"kind\":\"{}\",\"mm\":{},\"confidence\":{}}}", c.kind, mm(c.mm), c.confidence)).collect();
        let [x, y, w, h] = self.placed.map(round1);
        format!(
            "\"size\":{{\"kind\":{},\"mm\":{},\"confidence\":{},\"by\":\"{}\",\"applied\":{},\"candidates\":[{}]}},\"placed\":{{\"x\":{x},\"y\":{y},\"w\":{w},\"h\":{h}}},\"content\":{{\"x\":{x},\"y\":{y},\"w\":{w},\"h\":{h},\"px\":[{},{}]}},\"sheet\":{{\"mm\":[{},{}]}}",
            s.kind.map_or("null".into(), |k| format!("\"{k}\"")),
            s.mm.map_or("null".into(), mm),
            s.confidence,
            s.by,
            s.applied,
            cands.join(","),
            self.content.0,
            self.content.1,
            round1(self.sheet.0),
            round1(self.sheet.1)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The synthetic passport page of mrz.rs through the whole pipeline.
    fn run_page(place: Option<Place>) -> Layout {
        // the page on a dark desk, as it is photographed
        let g = crate::mrz::tests::page(1000, 704, 20.32, 44, 2);
        let (w, h, m) = (1400, 1100, 200);
        let mut px = vec![60u8; w * h * 3];
        for y in 0..g.h {
            for x in 0..g.w {
                let v = (g.d[y * g.w + x] * 255.0) as u8;
                let i = ((y + m) * w + x + m) * 3;
                px[i..i + 3].copy_from_slice(&[v, v, v]);
            }
        }
        let src = crate::img::Src { w, h, px };
        let mut o = crate::parse_args(&["--format".into(), "jpg".into(), "--dpi".into(), "200".into(), "x.png".into()]).unwrap();
        o.place = place;
        let done = crate::run(vec![crate::Source::new("x.png".into(), vec![src])], &o, &mut |_| {}, &|_, _| {}).unwrap();
        done.pages[0].layout.clone()
    }

    #[test]
    fn a_passport_page_at_its_real_size() {
        // 1000 px at 8 px/mm: 125 mm, by its zone; on a portrait A4, in the
        // middle, 15 mm from the top
        let l = run_page(None);
        assert_eq!((l.size.by, l.size.kind, l.size.applied), ("mrz", Some("id3"), true));
        let [x, y, w, _] = l.placed;
        assert!((w - 125.0).abs() < 3.0 && (y - TOP_MM).abs() < 0.2 && (x + w / 2.0 - 105.0).abs() < 1.0, "{:?}", l.placed);
        assert!((l.sheet.0 - 210.0).abs() < 0.5 && (l.sheet.1 - 297.0).abs() < 0.5, "{:?}", l.sheet);
        assert!(l.json_fields().contains("\"by\":\"mrz\""));
        // over the page when asked, as before
        let l = run_page(Some(Place::Fit));
        assert!(!l.size.applied && l.placed[2] > 190.0, "{:?}", l.placed);
        // where the person put it
        let l = run_page(Some(Place::At { w: 100.0, x: Some(10.0), y: Some(20.0) }));
        let [x, y, w, _] = l.placed;
        assert!((w - 100.0).abs() < 0.3 && (x - 10.0).abs() < 0.2 && (y - 20.0).abs() < 0.2, "{:?}", l.placed);
    }

    #[test]
    fn a_shape_alone_names_no_size() {
        // a page 1.414: A4, A5, A6 and a passport's page all fit
        let c = by_aspect(1000.0, 1414.0);
        assert!(c.iter().take(5).any(|c| c.kind == "a5") && c.iter().any(|c| c.kind == "id3"));
        assert!(c.iter().all(|c| c.confidence <= 0.5));
        assert_eq!(c.iter().find(|c| c.kind == "a5").unwrap().mm, [148.0, 210.0]);
        // ID-1's shape stands apart; the four always offered are there
        let c = by_aspect(856.0, 540.0);
        assert_eq!(c[0].kind, "id1");
        for k in ALWAYS {
            assert!(c.iter().any(|c| c.kind == k), "{k}");
        }
    }
}
