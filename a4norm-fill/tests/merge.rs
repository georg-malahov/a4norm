//! Several PDFs and a scanned page as one PDF (merge.rs): the pages are the
//! sources' own, shown pixel for pixel as each source shows them, their
//! text still text; an encrypted source opens, a form's fields stay drawn.
//! Needs poppler (pdftoppm, pdftotext, pdfinfo); without it, it passes and
//! says so.

use a4norm_fill::merge::{merge, Part};
use std::path::PathBuf;
use std::process::Command;

const FORMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/forms/");

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("a4norm-merge-{name}"))
}

/// Page `page` of a PDF at 50 dpi, as RGB bytes.
fn render(pdf: &[u8], page: u32, name: &str) -> Vec<u8> {
    let src = tmp(&format!("{name}.pdf"));
    std::fs::write(&src, pdf).unwrap();
    let out = tmp(&format!("{name}-{page}"));
    let p = page.to_string();
    let ok = Command::new("pdftoppm").args(["-r", "50", "-png", "-f", &p, "-l", &p, "-singlefile"]).arg(&src).arg(&out).status().unwrap();
    assert!(ok.success());
    image::open(out.with_extension("png")).unwrap().to_rgb8().into_raw()
}

fn poppler(tool: &str, pdf: &[u8], name: &str) -> String {
    let src = tmp(&format!("{name}.pdf"));
    std::fs::write(&src, pdf).unwrap();
    let mut c = Command::new(tool);
    if tool == "pdftotext" {
        c.arg(&src).arg("-");
    } else {
        c.arg(&src);
    }
    String::from_utf8_lossy(&c.output().unwrap().stdout).into_owned()
}

#[test]
fn pdfs_and_a_scan_as_one() {
    if Command::new("pdftoppm").arg("-v").output().is_err() {
        return eprintln!("skipped: no poppler");
    }
    let read = |f: &str| std::fs::read(format!("{FORMS}{f}")).unwrap();
    let (plain, encrypted, acroform) = (read("demo-blank.pdf"), read("demo-blank-encrypted.pdf"), read("demo-blank-acroform.pdf"));
    let kg1 = std::fs::read(format!("{FORMS}official/ba-kg1-kindergeld.pdf")).ok();
    let scan_pdf = read("demo-blank-a4norm-scan.pdf");
    let (i, j) = (scan_pdf.windows(2).position(|w| w == [0xFF, 0xD8]).unwrap(), scan_pdf.windows(2).rposition(|w| w == [0xFF, 0xD9]).unwrap());
    let jpg = scan_pdf[i..j + 2].to_vec();

    let mut parts = vec![
        Part::Pdf { pdf: &plain, pages: None },
        Part::Pdf { pdf: &encrypted, pages: None },
        Part::Jpeg { jpg: &jpg, dpi: Some(200.0), turn: 0, size_mm: None },
        Part::Pdf { pdf: &acroform, pages: None },
    ];
    // KG 1: pages 3 and 2, in that order (AES, owner password, static XFA)
    let mut want: Vec<(&[u8], u32)> = vec![(&plain, 1), (&encrypted, 1), (&scan_pdf, 1), (&acroform, 1)];
    if let Some(k) = &kg1 {
        parts.push(Part::Pdf { pdf: k, pages: Some(vec![3, 2]) });
        want.extend([(k.as_slice(), 3), (k.as_slice(), 2)]);
    }
    let out = merge(&parts, Some("Belege 2026 — Größe")).unwrap();
    let info = poppler("pdfinfo", &out, "out");
    assert!(info.contains(&format!("Pages:           {}", want.len())), "{info}");
    assert!(info.contains("Belege 2026 — Größe"), "{info}");
    assert!(!info.contains("Encrypted:       yes"), "{info}");
    // the text is still text
    let text = poppler("pdftotext", &out, "out");
    assert!(text.matches("Patientendaten und Datenschutzerklärung").count() >= 3, "{text}");
    // each page as its source shows it
    for (k, (src, page)) in want.iter().enumerate() {
        let (a, b) = (render(&out, k as u32 + 1, "out"), render(src, *page, &format!("src{k}")));
        assert_eq!(a.len(), b.len(), "page {}: the size", k + 1);
        let differ = a.iter().zip(&b).filter(|(x, y)| x.abs_diff(**y) > 2).count();
        eprintln!("page {}: {differ} of {} values differ", k + 1, a.len());
        assert!(differ * 1000 < a.len(), "page {}: {differ} values differ", k + 1);
    }
    // the files' sizes: the parts as they were, nothing rasterized
    let sum = plain.len() + encrypted.len() + jpg.len() + acroform.len() + kg1.as_ref().map_or(0, |k| k.len());
    eprintln!("{} bytes from {} (KG 1 whole)", out.len(), sum);
    assert!(out.len() < sum + 20_000);
}

#[test]
fn what_cannot_be_merged_says_which() {
    let plain = std::fs::read(format!("{FORMS}demo-blank.pdf")).unwrap();
    let e = merge(&[Part::Pdf { pdf: &plain, pages: None }, Part::Pdf { pdf: b"%PDF-1.4 not a PDF", pages: None }], None).unwrap_err();
    assert!(e.starts_with("part 2:"), "{e}");
    let e = merge(&[Part::Pdf { pdf: &plain, pages: Some(vec![2]) }], None).unwrap_err();
    assert!(e.contains("no page 2 of 1"), "{e}");
    assert!(merge(&[], None).is_err());
    let e = merge(&[Part::Jpeg { jpg: b"no", dpi: None, turn: 0, size_mm: None }], None).unwrap_err();
    assert!(e.starts_with("part 1: not a JPEG"), "{e}");
}
