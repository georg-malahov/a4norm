//! A page on its side or upside down is read the right way up: the demo's
//! phone scan and KG 1's page 2, turned a quarter, a half and three
//! quarters, give the inspection of the page as it is, and say how far
//! they were turned. Needs the models (and pdftoppm for KG 1); without
//! them it passes and says so.

use a4norm_ocr::{inspection_json, pdf_jpeg, Ocr};
use image::{imageops, RgbImage};
use std::process::Command;

const FORMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/forms/");

fn ocr() -> Option<Ocr> {
    let dir = std::env::var("A4NORM_OCR_MODELS").unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/models").into());
    let read = |f: &str| std::fs::read(format!("{dir}/{f}")).ok();
    let (det, rec, yml) = (read("det.onnx")?, read("rec.onnx")?, read("rec.yml")?);
    Some(Ocr::new(&det, &rec, std::str::from_utf8(&yml).unwrap()).unwrap())
}

/// The inspection's JSON, and its `orientation` taken out of it.
fn inspect(ocr: &Ocr, img: &RgbImage) -> (String, u16) {
    let size = if img.width() < img.height() { [595.28, 841.89] } else { [841.89, 595.28] };
    let (p, g) = ocr.inspect(img, size).unwrap();
    let json = inspection_json(&p, &g, size);
    (json.replace(&format!("\"orientation\":{},", p.orientation), ""), p.orientation)
}

#[test]
fn turned_pages_read_the_right_way_up() {
    let Some(ocr) = ocr() else {
        return eprintln!("skipped: no models (a4norm-ocr/models.sh)");
    };
    let bytes = std::fs::read(format!("{FORMS}demo-filled-scan.jpg")).unwrap();
    let mut pages = vec![("demo-filled-scan", image::load_from_memory(pdf_jpeg(&bytes).unwrap_or(&bytes)).unwrap().to_rgb8())];
    let png = std::env::temp_dir().join("a4norm-orientation-kg1-2");
    let made = Command::new("pdftoppm")
        .args(["-r", "200", "-png", "-f", "2", "-l", "2", "-singlefile", &format!("{FORMS}official/ba-kg1-kindergeld.pdf")])
        .arg(&png)
        .status()
        .is_ok_and(|s| s.success());
    if made {
        pages.push(("kg1-p2", image::open(png.with_extension("png")).unwrap().to_rgb8()));
    } else {
        eprintln!("skipped KG 1: no pdftoppm or no form here");
    }
    for (name, page) in pages {
        let (want, o) = inspect(&ocr, &page);
        assert_eq!(o, 0, "{name} as it is");
        assert!(want.contains("\"boxes\":[{"), "{name}: {want}");
        // turned clockwise by a quarter, it is read turned back by three
        for (turned, back) in [(imageops::rotate90(&page), 270), (imageops::rotate180(&page), 180), (imageops::rotate270(&page), 90)] {
            let (got, o) = inspect(&ocr, &turned);
            assert_eq!(o, back, "{name} turned {}", 360 - back);
            assert!(got == want, "{name} turned {}: not the same inspection", 360 - back);
        }
    }
}
