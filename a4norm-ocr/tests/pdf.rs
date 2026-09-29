//! The filled form as a PDF (pdf.rs): over the source's own pages, which
//! stay as they were (their text selectable, their fields' borders drawn),
//! the same from the encrypted and the AcroForm copies, a few kilobytes
//! more, nothing blue; and over a scan's picture.
//!
//! Needs the models (the pages are inspected first) and poppler (pdftoppm,
//! pdftotext); without them the tests pass and say so.

use a4norm_ocr::fill::{Inspection, Request};
use a4norm_ocr::{inspection_json, pdf, pdf_jpeg, Ocr};
use image::RgbImage;
use std::path::PathBuf;
use std::process::Command;

const FORMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/forms/");

fn ocr() -> Option<Ocr> {
    let dir = std::env::var("A4NORM_OCR_MODELS").unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/models").into());
    let read = |f: &str| std::fs::read(format!("{dir}/{f}")).ok();
    let (det, rec, yml) = (read("det.onnx")?, read("rec.onnx")?, read("rec.yml")?);
    Some(Ocr::new(&det, &rec, std::str::from_utf8(&yml).unwrap()).unwrap())
}

/// A file in the temp folder; `name` without dots (pdftoppm adds ".png").
fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("a4norm-pdf-{name}"))
}

/// Page `page` of a PDF at `dpi`, or None without pdftoppm.
fn render(pdf: &[u8], page: u32, dpi: u32, name: &str) -> Option<RgbImage> {
    let name = name.replace('.', "-");
    let src = tmp(&format!("{name}.pdf"));
    std::fs::write(&src, pdf).unwrap();
    let out = tmp(&format!("{name}-{page}"));
    let p = page.to_string();
    let ok = Command::new("pdftoppm")
        .args(["-r", &dpi.to_string(), "-png", "-f", &p, "-l", &p, "-singlefile"])
        .arg(&src)
        .arg(&out)
        .status()
        .is_ok_and(|s| s.success());
    ok.then(|| image::open(out.with_extension("png")).unwrap().to_rgb8())
}

fn text(pdf: &[u8], name: &str) -> String {
    let src = tmp(&format!("{}.pdf", name.replace('.', "-")));
    std::fs::write(&src, pdf).unwrap();
    let out = Command::new("pdftotext").arg(&src).arg("-").output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn request(ocr: &Ocr, img: &RgbImage, page: u32, template: &str, answers: &str) -> Request {
    let (p, g) = ocr.inspect(img, [595.28, 841.89]).unwrap();
    let mut ins: Inspection = serde_json::from_str(&inspection_json(&p, &g, [595.28, 841.89])).unwrap();
    ins.page = Some(page);
    let read = |f: &str| std::fs::read_to_string(format!("{FORMS}{f}")).unwrap();
    Request {
        inspections: vec![ins],
        template: serde_json::from_str(&read(template)).unwrap(),
        answers: serde_json::from_str(&read(answers)).unwrap(),
        color: None,
        min_size: None,
    }
}

/// Pixels that are clearly blue: blue over red and green by 40 levels.
fn blue(img: &RgbImage) -> usize {
    img.pixels().filter(|p| p[2] as i32 > p[0].max(p[1]) as i32 + 40).count()
}

fn ready() -> Option<Ocr> {
    let ocr = ocr();
    if ocr.is_none() {
        eprintln!("skipped: no models (a4norm-ocr/models.sh)");
    }
    let poppler = Command::new("pdftoppm").arg("-v").output().is_ok();
    if !poppler {
        eprintln!("skipped: no poppler");
    }
    ocr.filter(|_| poppler)
}

#[test]
fn the_demo_pdf_filled_over_its_own_page() {
    let Some(ocr) = ready() else { return };
    let source = std::fs::read(format!("{FORMS}demo-blank.pdf")).unwrap();
    let page = render(&source, 1, 200, "demo-source").unwrap();
    let req = request(&ocr, &page, 1, "demo-template.json", "demo-answers.json");
    let (_, out) = pdf::fill_pdf(&req, &source, &[]).unwrap();
    assert!(!out.fallback);
    // the form's own text is still text, and ours is added
    let t = text(&out.pdf, "demo-out");
    for s in ["Patientendaten und Datenschutzerklärung", "Überweisender Arzt", "Greenholt", "Becken/Hüfte li", "30.10.2025"] {
        assert!(t.contains(s), "{s} not in the text: {t}");
    }
    assert!(out.pdf.len() < source.len() + 16_000, "{} bytes from {}", out.pdf.len(), source.len());
    let shown = render(&out.pdf, 1, 100, "demo-out").unwrap();
    assert_eq!(blue(&shown), 0);
    // the encrypted copy (AES, empty user password, owner bans) and the one
    // with an AcroForm give the same page
    for copy in ["demo-blank-encrypted.pdf", "demo-blank-acroform.pdf"] {
        let src = std::fs::read(format!("{FORMS}{copy}")).unwrap();
        let (_, o) = pdf::fill_pdf(&req, &src, &[]).unwrap();
        assert!(!o.fallback && o.pdf.len() < src.len() + 16_000, "{copy}");
        assert_eq!(text(&o.pdf, copy), t, "{copy}: the text");
        assert!(render(&o.pdf, 1, 100, copy).unwrap() == shown, "{copy}: the page as shown");
    }
}

#[test]
fn the_phone_scan_filled_over_its_picture() {
    let Some(ocr) = ready() else { return };
    let bytes = std::fs::read(format!("{FORMS}demo-blank-a4norm-scan.pdf")).unwrap();
    let jpeg = pdf_jpeg(&bytes).unwrap();
    let img = image::load_from_memory(jpeg).unwrap().to_rgb8();
    let req = request(&ocr, &img, 1, "demo-template.json", "demo-answers.json");
    let (layout, out) = pdf::fill_scan(&req, &[jpeg]).unwrap();
    assert_eq!(layout.placed.len(), 14);
    // the picture as it was, and the answers as text
    assert!(out.pdf.len() < jpeg.len() + 16_000);
    let t = text(&out.pdf, "scan-out");
    assert!(t.contains("Greenholt") && t.contains("PRIVAT (Continentale)"), "{t}");
    assert_eq!(blue(&render(&out.pdf, 1, 100, "scan-out").unwrap()), 0);
}

#[test]
fn an_unreadable_pdf_falls_back_to_its_pictures() {
    let Some(ocr) = ready() else { return };
    let bytes = std::fs::read(format!("{FORMS}demo-blank-a4norm-scan.pdf")).unwrap();
    let jpeg = pdf_jpeg(&bytes).unwrap();
    let img = image::load_from_memory(jpeg).unwrap().to_rgb8();
    let req = request(&ocr, &img, 1, "demo-template.json", "demo-answers.json");
    let (_, out) = pdf::fill_pdf(&req, b"%PDF-1.4 not a PDF", &[jpeg]).unwrap();
    assert!(out.fallback);
    assert!(pdf::fill_pdf(&req, b"%PDF-1.4 not a PDF", &[]).is_err());
}

#[test]
fn kindergeld_page_two() {
    let Some(ocr) = ready() else { return };
    let Ok(source) = std::fs::read(format!("{FORMS}official/ba-kg1-kindergeld.pdf")) else {
        return eprintln!("skipped: no KG 1 here");
    };
    let page = render(&source, 2, 200, "kg1-source").unwrap();
    let req = request(&ocr, &page, 2, "kg1-p2-template.json", "kg1-p2-answers.json");
    let (layout, out) = pdf::fill_pdf(&req, &source, &[]).unwrap();
    let placed = |k: &str| layout.placed.iter().find(|p| p.key == k).unwrap_or_else(|| panic!("{k}"));
    // the tax ID through its four combs, 2 + 3 + 3 + 3; the IBAN in its cells
    assert!(placed("tax_id").kind == "comb" && !placed("tax_id").overflow && placed("tax_id").lines == 4);
    assert!(placed("iban").kind == "comb" && !placed("iban").overflow);
    assert_eq!(layout.placed.iter().filter(|p| p.overflow).count(), 0);
    // AES and owner bans gone, the five pages kept; nothing of page 2 lost:
    // every dark pixel of the form as it was shown is dark on the filled one
    // (its comb cells are its fields' borders)
    let t = text(&out.pdf, "kg1-out");
    assert!(t.contains("Antrag auf Kindergeld") && t.contains("Musterfrau") && t.contains("Musterbank Berlin"));
    let before = render(&source, 2, 100, "kg1-source").unwrap();
    let after = render(&out.pdf, 2, 100, "kg1-out").unwrap();
    let lost = before.pixels().zip(after.pixels()).filter(|(a, b)| a[0].max(a[1]).max(a[2]) < 100 && b[0].min(b[1]).min(b[2]) > 180).count();
    assert!(lost < 20, "{lost} dark pixels of the form gone");
    assert_eq!(blue(&after), 0);
    assert!(render(&out.pdf, 5, 50, "kg1-out").is_some(), "page 5");
}
