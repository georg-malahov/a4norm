//! Languages of official forms' first pages: a label in several languages
//! gives all of them. Needs the models, the forms (examples/forms/official.py)
//! and poppler's pdftoppm to render them; without any of these, it passes
//! and says so.

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
