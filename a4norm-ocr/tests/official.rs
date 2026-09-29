//! Official forms' first pages: a label in several languages gives all of
//! them, and check boxes read with their labels stay boxes. Needs the models, the forms (examples/forms/official.py)
//! and poppler's pdftoppm to render them; without any of these, it passes
//! and says so.

use a4norm_ocr::geometry::Kind;
use a4norm_ocr::Ocr;
use std::process::Command;

#[test]
fn multilingual_labels() {
    let dir = std::env::var("A4NORM_OCR_MODELS").unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/models").into());
    let read = |f: &str| std::fs::read(format!("{dir}/{f}")).ok();
    let (Some(det), Some(rec), Some(yml)) = (read("det.onnx"), read("rec.onnx"), read("rec.yml")) else {
        return eprintln!("skipped: no models (a4norm-ocr/models.sh)");
    };
    let ocr = Ocr::new(&det, &rec, std::str::from_utf8(&yml).unwrap()).unwrap();
    let forms = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/forms/official/");
    let tmp = std::env::temp_dir();
    for (form, want) in [
        ("berlin-aufenthaltstitel", &["de", "en", "fr", "it"][..]),
        ("frankfurt-aufenthaltstitel", &["de", "en", "fr", "es", "hr", "tr"][..]),
        ("muenchen-aufenthaltstitel", &["de", "en"][..]),
        ("ba-kg1-kindergeld", &["de"][..]),
    ] {
        let pdf = format!("{forms}{form}.pdf");
        let png = tmp.join(format!("a4norm-ocr-{form}"));
        let made = std::path::Path::new(&pdf).exists()
            && Command::new("pdftoppm")
                .args(["-r", "200", "-png", "-f", "1", "-l", "1", "-singlefile", &pdf])
                .arg(&png)
                .status()
                .is_ok_and(|s| s.success());
        if !made {
            eprintln!("skipped {form}: no form here, or no pdftoppm");
            continue;
        }
        let img = image::open(png.with_extension("png")).unwrap().to_rgb8();
        let langs = ocr.page(&img).unwrap().langs;
        eprintln!("{form}: {langs:?}");
        for l in want {
            assert!(langs.contains(l), "{form}: {l} not in {langs:?}");
        }
        assert!(langs.len() <= want.len() + 1, "{form}: {langs:?}");
    }
}

/// Page `page` of an official form at `dpi`, inspected: the boxes' left
/// edges and tops in points, and the words; None without the form or
/// pdftoppm.
fn boxes_and_words(ocr: &Ocr, form: &str, page: u32, dpi: u32) -> Option<(Vec<[f32; 2]>, Vec<String>)> {
    let pdf = format!("{}/../examples/forms/official/{form}.pdf", env!("CARGO_MANIFEST_DIR"));
    let png = std::env::temp_dir().join(format!("a4norm-ocr-{form}-{page}-{dpi}"));
    let p = page.to_string();
    let made = std::path::Path::new(&pdf).exists()
        && Command::new("pdftoppm")
            .args(["-r", &dpi.to_string(), "-png", "-f", &p, "-l", &p, "-singlefile", &pdf])
            .arg(&png)
            .status()
            .is_ok_and(|s| s.success());
    if !made {
        eprintln!("skipped {form}: no form here, or no pdftoppm");
        return None;
    }
    let img = image::open(png.with_extension("png")).unwrap().to_rgb8();
    let (p, g) = ocr.inspect(&img, [595.28, 841.89]).unwrap();
    let px_pt = img.width() as f32 / 595.28;
    let boxes = g
        .candidates
        .iter()
        .filter_map(|c| if let Kind::Box(b) = c.kind { Some([b[0] / px_pt, b[1] / px_pt]) } else { None })
        .collect();
    Some((boxes, p.lines.iter().flat_map(|l| &l.words).map(|w| w.text.clone()).collect()))
}

#[test]
fn boxes_read_with_their_labels() {
    let dir = std::env::var("A4NORM_OCR_MODELS").unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/models").into());
    let read = |f: &str| std::fs::read(format!("{dir}/{f}")).ok();
    let (Some(det), Some(rec), Some(yml)) = (read("det.onnx"), read("rec.onnx"), read("rec.yml")) else {
        return eprintln!("skipped: no models (a4norm-ocr/models.sh)");
    };
    let ocr = Ocr::new(&det, &rec, std::str::from_utf8(&yml).unwrap()).unwrap();
    let row = |boxes: &[[f32; 2]], y: f32| boxes.iter().filter(|b| (b[1] - y).abs() < 3.0).map(|b| b[0]).collect::<Vec<_>>();
    // Jobcenter Hauptantrag p. 1, "Geschlecht": "□ männlich □ weiblich
    // □ divers □ keine Angabe" at 300 dpi reads as one word over the boxes;
    // nothing is read in them, so they are boxes, not letters' loops, and
    // part the word into the labels
    if let Some((boxes, words)) = boxes_and_words(&ocr, "ba-jobcenter-hauptantrag", 1, 300) {
        assert_eq!(row(&boxes, 552.5).len(), 4, "Geschlecht boxes at x {:?}", row(&boxes, 552.5));
        let run = ["männlich", "weiblich", "divers", "keine"];
        assert!(words.windows(4).any(|w| w == run), "{run:?} in {words:?}");
    }
    // Familiengeld p. 1: "◯ ja ◯ nein", each circle read as a letter with
    // its label ("Oja"), a word space after it
    if let Some((boxes, _)) = boxes_and_words(&ocr, "zbfs-familiengeld", 1, 200) {
        assert_eq!(row(&boxes, 669.8).len(), 2, "ja/nein at x {:?}", row(&boxes, 669.8));
    }
    // Frankfurt p. 3: the "O" of "Ocupación" is a letter, its "c" close by
    if let Some((boxes, _)) = boxes_and_words(&ocr, "frankfurt-aufenthaltstitel", 3, 200) {
        assert!(row(&boxes, 421.1).iter().all(|x| (x - 91.1).abs() > 2.0), "{:?}", row(&boxes, 421.1));
    }
}
