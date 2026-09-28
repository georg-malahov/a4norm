//! The demo form read back: every line of its text found, at most 1 % of
//! the characters wrong, German, and the print size of the page.
//!
//! Needs the models (a4norm-ocr/models.sh); without them the tests say so
//! and pass. The truth is the text of the PDFs the pages came from
//! (examples/forms/demo-*.txt).

use a4norm_ocr::{pdf_jpeg, Ocr, Page};
use std::collections::HashMap;
use std::sync::OnceLock;

const FORMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/forms/");

fn ocr() -> Option<&'static Ocr> {
    static OCR: OnceLock<Option<Ocr>> = OnceLock::new();
    OCR.get_or_init(|| {
        let dir = std::env::var("A4NORM_OCR_MODELS").unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/models").into());
        let read = |f: &str| std::fs::read(format!("{dir}/{f}")).ok();
        let (det, rec, yml) = (read("det.onnx")?, read("rec.onnx")?, read("rec.yml")?);
        Some(Ocr::new(&det, &rec, std::str::from_utf8(&yml).unwrap()).unwrap())
    })
    .as_ref()
}

fn page(name: &str) -> Page {
    let bytes = std::fs::read(format!("{FORMS}{name}")).unwrap();
    let img = image::load_from_memory(pdf_jpeg(&bytes).unwrap_or(&bytes)).unwrap().to_rgb8();
    ocr().unwrap().page(&img).unwrap()
}

/// Characters wrong, in any order of lines: the count of characters (spaces
/// aside) that one text has more of than the other, the larger side. A
/// character read as another counts once.
fn wrong(truth: &str, read: &str) -> usize {
    let mut n: HashMap<char, i64> = HashMap::new();
    truth.chars().filter(|c| !c.is_whitespace()).for_each(|c| *n.entry(c).or_default() += 1);
    read.chars().filter(|c| !c.is_whitespace()).for_each(|c| *n.entry(c).or_default() -= 1);
    let missing: i64 = n.values().filter(|&&v| v > 0).sum();
    let extra: i64 = -n.values().filter(|&&v| v < 0).sum::<i64>();
    missing.max(extra) as usize
}

/// The truth's lines that the page lacks: those with fewer than half of
/// their words read exactly.
fn lost_lines(truth: &str, p: &Page) -> Vec<String> {
    // "Anmerkungen" is read with the rule it runs into, "Anmerkungen-"
    let bare = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
    let have: std::collections::HashSet<String> = p.lines.iter().flat_map(|l| &l.words).map(|w| bare(&w.text)).collect();
    truth
        .lines()
        .filter(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let hit = words.iter().filter(|w| have.contains(&bare(w))).count();
            2 * hit < words.len()
        })
        .map(str::to_string)
        .collect()
}

fn check(image: &str, truth: &str) -> Page {
    let truth = std::fs::read_to_string(format!("{FORMS}{truth}")).unwrap();
    let p = page(image);
    let read: Vec<String> = p.lines.iter().map(|l| l.text()).collect();
    let total = truth.chars().filter(|c| !c.is_whitespace()).count();
    let bad = wrong(&truth, &read.join("\n"));
    let lost = lost_lines(&truth, &p);
    eprintln!(
        "{image}: {} lines, {bad} of {total} characters wrong ({:.2} %), skew {:.2}°, lost {lost:?}",
        p.lines.len(),
        100.0 * bad as f64 / total as f64,
        p.skew.to_degrees()
    );
    assert!(lost.is_empty(), "{image}: lines not found: {lost:?}");
    assert!(bad * 100 <= total, "{image}: {bad} of {total} characters wrong");
    assert_eq!(p.langs, ["de"], "{image}");
    p
}

#[test]
fn the_filled_scan() {
    if ocr().is_none() {
        return eprintln!("skipped: no models (a4norm-ocr/models.sh)");
    }
    let p = check("demo-filled-scan.jpg", "demo-filled.txt");
    // turned 0.6° when it was made
    assert!((p.skew.to_degrees() - 0.6).abs() < 0.15, "skew {}", p.skew.to_degrees());
    // the values, each a word of its own
    for v in ["Greenholt", "Henry", "20.08.1954", "Harbors", "+493023125042", "(Continentale)", "Elfrieda"] {
        assert!(p.lines.iter().flat_map(|l| &l.words).any(|w| w.text == v), "{v}");
    }
}

#[test]
fn the_real_scan() {
    if ocr().is_none() {
        return eprintln!("skipped: no models (a4norm-ocr/models.sh)");
    }
    let p = check("demo-blank-a4norm-scan.pdf", "demo-blank.txt");
    // most of its text is set at 10 to 12 pt: the print size in points
    let pt = p.printed * 841.89 / p.height as f32;
    assert!((9.0..=13.0).contains(&pt), "print {pt:.1} pt");
}

#[test]
fn words_sit_on_the_page() {
    if ocr().is_none() {
        return eprintln!("skipped: no models (a4norm-ocr/models.sh)");
    }
    let p = page("demo-filled-scan.jpg");
    let json = p.to_json([595.28, 841.89]);
    // "Greenholt" is at 73–143 pt across, 86–108 pt down in the PDF; the
    // scan is turned 0.6° about the middle, which moves it by ~3 pt
    let w = p.lines.iter().flat_map(|l| &l.words).find(|w| w.text == "Greenholt").unwrap();
    let s = 72.0 / 200.0;
    let b = w.bbox.map(|v| v * s);
    assert!((b[0] - 73.1).abs() < 8.0 && (b[2] - 143.4).abs() < 8.0, "{b:?}");
    assert!(b[1] < 97.0 && b[3] > 97.0, "{b:?}");
    assert!(json.starts_with("{\"sizePt\":[595.28,841.89],\"skewDeg\":0.6"), "{}", &json[..60]);
}
