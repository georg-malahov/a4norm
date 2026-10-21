//! A field's style (fill.rs `Style`): bold set in Arimo Bold, underline a
//! rule under the text. Without a style the PDF is byte for byte what it was
//! before styles were there. Synthetic pages only: a page drawn here, and the
//! public demo form (examples/forms/demo-blank.pdf).

use a4norm_fill::fill::{self, Mark, Request, Style};
use a4norm_fill::{font, pdf};

const FORMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/forms/");

/// A page as the scanner's geometry would describe it: a writing line, a
/// field, a comb and a box, and a free text.
fn request(style: &str) -> Request {
    let json = format!(
        r##"{{
        "inspections": [{{"page": 1, "sizePt": [595.28, 841.89], "printedSize": 9, "typicalFieldHeight": 18,
          "lines": [{{"id": 1, "x0": 60, "y0": 200, "x1": 300, "y1": 200}}],
          "rects": [{{"id": 2, "box": [60, 260, 400, 290]}}],
          "combs": [{{"id": 3, "box": [60, 330, 260, 350], "cells": 10}}],
          "boxes": [{{"id": 4, "box": [60, 380, 72, 392]}}]}}],
        "template": {{"fields": [
          {{"key": "name", "place": {{"candidate": 1}}{style}}},
          {{"key": "note", "place": {{"candidate": 2}}{style}}},
          {{"key": "id", "type": "comb", "place": {{"candidate": 3}}{style}}},
          {{"key": "ok", "type": "choice", "place": {{"candidate": 4}}{style}}}]}},
        "answers": {{"name": "Şahin Yılmaz", "note": "NEIN – nicht einverstanden", "id": "1234567890", "ok": true}},
        "texts": [{{"page": 1, "x": 60, "y": 500, "size": 11, "text": "Heute: 07.10.2026 — Größe ş ł", "rotate": 10}}]
      }}"##
    );
    serde_json::from_str(&json).unwrap()
}

/// A white page, as a JPEG.
fn blank_jpeg() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(620, 877, image::Rgb([255, 255, 255]));
    let mut out = std::io::Cursor::new(vec![]);
    img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

/// The bytes' FNV-1a 64 fingerprint, with their length.
fn print(b: &[u8]) -> String {
    let h = b.iter().fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3));
    format!("{} {:016x}", b.len(), h)
}

fn scan_pdf(req: &Request) -> Vec<u8> {
    pdf::fill_scan(req, &[&blank_jpeg()], &[]).unwrap().1.pdf
}

fn over_pdf(req: &Request) -> Vec<u8> {
    let src = std::fs::read(format!("{FORMS}demo-blank.pdf")).unwrap();
    pdf::fill_pdf(req, &src, &[], &[]).unwrap().1.pdf
}

/// What `main` wrote for this request before styles were there (built and
/// fingerprinted at 87b5b11 and 0452a73).
const SCAN_BEFORE: &str = "24997 de25821a9b1145f9";
const OVER_BEFORE: &str = "13690 370ba56c54d2f5ed";

#[test]
fn no_style_is_as_before() {
    assert_eq!(print(&scan_pdf(&request(""))), SCAN_BEFORE);
    assert_eq!(print(&over_pdf(&request(""))), OVER_BEFORE);
    for none in [r#", "style": {}"#, r#", "style": {"bold": false, "underline": false}"#] {
        assert_eq!(print(&scan_pdf(&request(none))), SCAN_BEFORE, "{none}");
        assert_eq!(print(&over_pdf(&request(none))), OVER_BEFORE, "{none}");
    }
}

/// Page 1's content, its streams inflated, as text.
fn content(pdf: &[u8]) -> String {
    let doc = lopdf::Document::load_mem(pdf).unwrap();
    let page = *doc.get_pages().get(&1).unwrap();
    String::from_utf8_lossy(&doc.get_page_content(page)).into_owned()
}

/// The text marks of a request's layout, with their style.
fn texts(req: &Request) -> Vec<(f32, f32, f32, f32, String, Style)> {
    fill::layout(req)
        .marks
        .into_iter()
        .filter_map(|m| match m {
            Mark::Text { x, y, size, angle, text, style, .. } => Some((x, y, size, angle, text, style)),
            _ => None,
        })
        .collect()
}

#[test]
fn bold_is_set_in_arimo_bold() {
    let req = request(r#", "style": {"bold": true}"#);
    for t in texts(&req).iter().filter(|t| t.4 != "Heute: 07.10.2026 — Größe ş ł") {
        assert!(t.5.bold, "{:?}", t);
    }
    for pdf in [scan_pdf(&req), over_pdf(&req)] {
        let raw = String::from_utf8_lossy(&pdf);
        // both fonts: the free text is not styled, the fields are
        assert!(raw.contains("+Arimo-Bold") && raw.contains("+Arimo-Regular"));
        let c = content(&pdf);
        assert!(c.contains("/A4nFontB") && c.contains("/A4nFont "), "{c}");
        // no rule without underline
        assert!(!c.contains(" 0 J "), "{c}");
        // copies as written (over the demo form its own words come between)
        if let Some(t) = pdftotext(&pdf) {
            for s in ["Şahin", "Yılmaz", "NEIN", "–", "einverstanden"] {
                assert!(t.contains(s), "{s} in {t}");
            }
        }
    }
    // no bold anywhere: no Bold embedded
    assert!(!String::from_utf8_lossy(&scan_pdf(&request(r#", "style": {"underline": true}"#))).contains("Arimo-Bold"));
}

#[test]
fn a_bold_value_is_fitted_by_its_own_widths() {
    // a value that just fits its field in the Regular is too wide in the
    // Bold: it is set smaller, or broken
    let long = "Bitte nicht verwenden – Allergie gegen Kontrastmittel";
    // the field as wide as the value in the Regular at the base size, with
    // its pads and a point to spare
    let base = fill::base_size(&request("").inspections);
    let wide = font::width(long, base) + 2.0 * 2.0 + 1.0;
    let fit = |style: &str| {
        let mut req = request(style);
        req.inspections[0].rects[0].b[2] = 60.0 + wide;
        req.answers.insert("note".into(), serde_json::Value::String(long.into()));
        let t = texts(&req);
        let rows: Vec<_> = t.iter().filter(|m| long.contains(&m.4) && m.4.len() > 3).cloned().collect();
        let w = rows.iter().map(|m| font::width_of(m.5.bold, &m.4, m.2)).fold(0.0, f32::max);
        (rows[0].2, rows.len(), w)
    };
    let (rs, rn, rw) = fit("");
    let (bs, bn, bw) = fit(r#", "style": {"bold": true}"#);
    assert!(rw <= wide - 4.0 && bw <= wide - 4.0, "{rw} {bw} in {wide}");
    assert!(rs == base && rn == 1, "the Regular fits as it is: {rs} pt x {rn}");
    assert!(bs < rs || bn > rn, "regular {rs} pt x {rn}, bold {bs} pt x {bn}");
}

#[test]
fn underline_runs_under_each_line_as_wide_as_it() {
    let long = "NEIN – nicht einverstanden mit der Gabe von jodhaltigem Kontrastmittel bei der Untersuchung";
    let mut req = request(r#", "style": {"bold": true, "underline": true}"#);
    // made smaller (and broken) to fit: the rule goes with the size it took
    req.answers.insert("note".into(), serde_json::Value::String(long.into()));
    let marks = texts(&req);
    let c = content(&scan_pdf(&req));
    let rules: Vec<[f32; 5]> = c
        .lines()
        .filter(|l| l.starts_with("q ") && l.contains(" w 0 J "))
        .map(|l| {
            let v: Vec<f32> = l.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            // q W w 0 J x0 y0 m x1 y1 l S Q
            [v[0], v[2], v[3], v[4], v[5]]
        })
        .collect();
    let styled: Vec<_> = marks.iter().filter(|m| m.5.underline).collect();
    assert_eq!(rules.len(), styled.len(), "a rule a line: {c}");
    assert!(styled.iter().any(|m| m.2 < fill::base_size(&req.inspections)), "the note was made smaller");
    for (m, r) in styled.iter().zip(&rules) {
        let (x, y, size, angle) = (m.0, m.1, m.2, m.3);
        let (sin, cos) = angle.sin_cos();
        let w = font::width_of(true, &m.4, size);
        let near = |a: f32, b: f32| (a - b).abs() < 0.011;
        assert!(near(r[0], size / 18.0), "thickness {} for {size}", r[0]);
        assert!(near(r[1], x - 0.08 * size * sin) && near(r[2], y + 0.08 * size * cos), "start {r:?} under {x},{y}");
        assert!(near(r[3], r[1] + w * cos) && near(r[4], r[2] + w * sin), "as long as {w}: {r:?}");
    }
}

/// The plain and the styled demo form as PDFs, for a look:
/// `A4NORM_STYLE_OUT=DIR cargo test --test style show`.
#[test]
fn show() {
    if let Ok(dir) = std::env::var("A4NORM_STYLE_OUT") {
        let mut req = request(r#", "style": {"bold": true, "underline": true}"#);
        req.answers.insert("note".into(), serde_json::Value::String("NEIN – nicht einverstanden".into()));
        std::fs::write(format!("{dir}/styled.pdf"), over_pdf(&req)).unwrap();
        std::fs::write(format!("{dir}/plain.pdf"), over_pdf(&request(""))).unwrap();
        std::fs::write(format!("{dir}/styled-scan.pdf"), scan_pdf(&req)).unwrap();
        let mut plain = request("");
        plain.answers.insert("note".into(), serde_json::Value::String("NEIN – nicht einverstanden".into()));
        std::fs::write(format!("{dir}/plain-scan.pdf"), scan_pdf(&plain)).unwrap();
        // for the browser module's check: the page, and the requests as JSON
        std::fs::write(format!("{dir}/blank.jpg"), blank_jpeg()).unwrap();
        std::fs::write(format!("{dir}/plain.pdf-scan"), scan_pdf(&request(""))).unwrap();
        std::fs::write(format!("{dir}/styled.pdf-scan"), scan_pdf(&request(r#", "style": {"bold": true, "underline": true}"#))).unwrap();
    }
}

#[test]
fn underline_is_drawn_under_the_text() {
    // rendered: under the note's baseline a dark row as wide as the text,
    // and none without the style
    let Some(png) = render(&scan_pdf(&request(r#", "style": {"underline": true}"#)), "ul") else { return };
    let Some(plain) = render(&scan_pdf(&request("")), "plain") else { return };
    let req = request(r#", "style": {"underline": true}"#);
    let note = texts(&req).into_iter().find(|m| m.4.starts_with("NEIN")).unwrap();
    let k = 100.0 / 72.0;
    let w = font::width(&note.4, note.2);
    let row = ((note.1 + 0.08 * note.2) * k).round() as u32;
    let dark = |img: &image::GrayImage, y: u32| {
        (((note.0 + 1.0) * k) as u32..((note.0 + w - 1.0) * k) as u32).filter(|&x| (y - 1..=y + 1).any(|yy| img.get_pixel(x, yy)[0] < 128)).count()
    };
    let span = ((w - 2.0) * k) as usize;
    assert!(dark(&png, row) * 10 >= span * 9, "{} of {span}", dark(&png, row));
    assert!(dark(&plain, row) * 2 < span, "{} of {span} without the style", dark(&plain, row));
}

fn pdftotext(pdf: &[u8]) -> Option<String> {
    let p = std::env::temp_dir().join(format!("a4norm-style-{}.pdf", pdf.len()));
    std::fs::write(&p, pdf).unwrap();
    let o = std::process::Command::new("pdftotext").arg(&p).arg("-").output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Page 1 at 100 dpi, grey; None without poppler.
fn render(pdf: &[u8], name: &str) -> Option<image::GrayImage> {
    let src = std::env::temp_dir().join(format!("a4norm-style-{name}.pdf"));
    std::fs::write(&src, pdf).unwrap();
    let out = std::env::temp_dir().join(format!("a4norm-style-{name}"));
    let ok = std::process::Command::new("pdftoppm").args(["-r", "100", "-gray", "-png", "-singlefile"]).arg(&src).arg(&out).status().is_ok_and(|s| s.success());
    if !ok {
        eprintln!("skipped: no poppler");
    }
    ok.then(|| image::open(out.with_extension("png")).unwrap().to_luma8())
}
