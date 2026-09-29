//! Where the answers go (fill.rs): the demo form's 12 values at their
//! printed places on its three pictures, and the rules of the sizes.

use a4norm_ocr::fill::{self, Inspection, Mark, Request, Template};
use a4norm_ocr::{helvetica, inspection_json, pdf_jpeg, Ocr};
use image::RgbImage;
use serde_json::json;
use std::process::Command;

const FORMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/forms/");
const A4: [f32; 2] = [595.28, 841.89];

fn ocr() -> Option<Ocr> {
    let dir = std::env::var("A4NORM_OCR_MODELS").unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/models").into());
    let read = |f: &str| std::fs::read(format!("{dir}/{f}")).ok();
    let (det, rec, yml) = (read("det.onnx")?, read("rec.onnx")?, read("rec.yml")?);
    Some(Ocr::new(&det, &rec, std::str::from_utf8(&yml).unwrap()).unwrap())
}

fn picture(name: &str) -> Option<RgbImage> {
    if name.ends_with(".pdf") && name == "demo-blank.pdf" {
        let out = std::env::temp_dir().join("a4norm-fill-demo-blank");
        let ok = Command::new("pdftoppm")
            .args(["-r", "200", "-png", "-singlefile", &format!("{FORMS}{name}")])
            .arg(&out)
            .status()
            .is_ok_and(|s| s.success());
        return ok.then(|| image::open(out.with_extension("png")).unwrap().to_rgb8());
    }
    let bytes = std::fs::read(format!("{FORMS}{name}")).unwrap();
    Some(image::load_from_memory(pdf_jpeg(&bytes).unwrap_or(&bytes)).unwrap().to_rgb8())
}

fn request(ins: Inspection) -> Request {
    let template: Template = serde_json::from_str(&std::fs::read_to_string(format!("{FORMS}demo-template.json")).unwrap()).unwrap();
    let answers = serde_json::from_str(&std::fs::read_to_string(format!("{FORMS}demo-answers.json")).unwrap()).unwrap();
    Request { inspections: vec![ins], template, answers, color: None, min_size: None }
}

/// The candidates' ends, to lay one picture on another.
fn ends(ins: &Inspection) -> Vec<(f64, f64)> {
    let mut v: Vec<(usize, [f32; 4])> = ins.lines.iter().map(|l| (l.id, [l.x0, l.y0, l.x1, l.y1])).collect();
    v.extend(ins.boxes.iter().map(|b| (b.id, b.b)));
    v.sort_by_key(|p| p.0);
    v.iter().flat_map(|(_, e)| [(e[0] as f64, e[1] as f64), (e[2] as f64, e[3] as f64)]).collect()
}

/// The turn, scale and shift that lays `p` on `q` best, as a map.
fn fit(p: &[(f64, f64)], q: &[(f64, f64)]) -> impl Fn(f64, f64) -> (f64, f64) {
    let n = p.len() as f64;
    let mean = |v: &[(f64, f64)]| v.iter().fold((0.0, 0.0), |s, a| (s.0 + a.0 / n, s.1 + a.1 / n));
    let (mp, mq) = (mean(p), mean(q));
    let (mut re, mut im, mut den) = (0.0, 0.0, 0.0);
    for (u, v) in p.iter().zip(q) {
        let (ux, uy, vx, vy) = (u.0 - mp.0, u.1 - mp.1, v.0 - mq.0, v.1 - mq.1);
        re += ux * vx + uy * vy;
        im += ux * vy - uy * vx;
        den += ux * ux + uy * uy;
    }
    let (a, b) = (re / den, im / den);
    move |x, y| {
        let (ux, uy) = (x - mp.0, y - mp.1);
        (a * ux - b * uy + mq.0, b * ux + a * uy + mq.1)
    }
}

#[test]
fn the_demo_values_where_they_were_printed() {
    let Some(ocr) = ocr() else {
        return eprintln!("skipped: no models (a4norm-ocr/models.sh)");
    };
    let inspect = |img: &RgbImage| -> Inspection {
        let (p, g) = ocr.inspect(img, A4).unwrap();
        serde_json::from_str(&inspection_json(&p, &g, A4)).unwrap()
    };
    // the truth: each value's box in the filled PDF; its baseline 0.299 of
    // its size above the box's foot, its size the box's height / 1.374
    let truth: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(format!("{FORMS}demo-truth.json")).unwrap()).unwrap();
    let answers: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(format!("{FORMS}demo-answers.json")).unwrap()).unwrap();
    let Some(pdf) = picture("demo-blank.pdf") else {
        return eprintln!("skipped: no pdftoppm for the PDF's own picture");
    };
    let reference = inspect(&pdf);
    for name in ["demo-blank.pdf", "demo-blank-scan.jpg", "demo-blank-a4norm-scan.pdf"] {
        let ins = if name == "demo-blank.pdf" { reference.clone() } else { inspect(&picture(name).unwrap()) };
        let to_here = fit(&ends(&reference), &ends(&ins));
        let l = fill::layout(&request(ins));
        assert_eq!(l.base, 13.0, "{name}: 11 pt print + 2, fields ~32 pt high");
        let mut worst = 0f64;
        for t in &truth {
            let text = t["text"].as_str().unwrap();
            let key = answers.iter().find(|(_, v)| v.as_str() == Some(text)).unwrap().0;
            let b: Vec<f64> = t["bbox"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            let size = (b[3] - b[1]) / 1.374;
            let (tx, ty) = to_here(b[0], b[3] - 0.299 * size);
            let p = l.placed.iter().find(|p| &p.key == key).unwrap_or_else(|| panic!("{name}: {key} not placed"));
            let d = ((p.x as f64 - tx).powi(2) + (p.y as f64 - ty).powi(2)).sqrt();
            assert!(d <= 6.0, "{name}: {key} at {:.1},{:.1}, printed at {tx:.1},{ty:.1}", p.x, p.y);
            assert!(!p.overflow && p.lines == 1 && p.size == 13.0, "{name}: {p:?}");
            worst = worst.max(d);
        }
        // "nein" to the e-mail, "ja" to Beihilfe (its options' boxes by row)
        let checks: Vec<_> = l.placed.iter().filter(|p| p.kind == "check").map(|p| (p.key.as_str(), p.candidate)).collect();
        assert_eq!(checks, [("email_consent", Some(11)), ("beihilfe", Some(13))], "{name}");
        eprintln!("{name}: 12 values within {worst:.1} pt of where they were printed");
    }
}

/// A page with one field of each kind, in points.
fn page() -> Inspection {
    serde_json::from_value(json!({
        "sizePt": A4, "printedSize": 9.0, "typicalFieldHeight": 16.0,
        "words": [{"text": "Anschrift", "bbox": [52.0, 101.0, 90.0, 109.0]}],
        "lines": [{"id": 1, "x0": 50.0, "y0": 60.0, "x1": 250.0, "y1": 60.0}],
        "rects": [{"id": 2, "box": [50.0, 100.0, 250.0, 130.0]}, {"id": 4, "box": [300.0, 100.0, 400.0, 112.0]}],
        "combs": [{"id": 3, "box": [50.0, 150.0, 250.0, 166.0], "cells": 11}],
        "boxes": [{"id": 5, "box": [300.0, 150.0, 310.0, 160.0]}]
    }))
    .unwrap()
}

fn place(field: &str, id: usize, value: &str) -> Request {
    Request {
        inspections: vec![page()],
        template: serde_json::from_value(json!({"fields": [{"key": field, "place": {"page": 1, "candidate": id}}]})).unwrap(),
        answers: [(field.to_string(), json!(value))].into(),
        color: None,
        min_size: None,
    }
}

/// Every text mark's extent: `[x0, top, x1, foot]`.
fn extents(l: &fill::Layout) -> Vec<[f32; 4]> {
    l.marks
        .iter()
        .filter_map(|m| match m {
            Mark::Text { x, y, size, text, .. } => {
                Some([*x, y - helvetica::CAP * size, x + helvetica::width(text, *size), y + helvetica::DESCENT * size])
            }
            _ => None,
        })
        .collect()
}

#[test]
fn short_values_at_the_base_size() {
    // 9 pt print + 2 = 11, under 0.72 x 16
    let l = fill::layout(&place("name", 1, "Greenholt"));
    assert_eq!(l.base, 11.0);
    assert_eq!((l.placed[0].size, l.placed[0].lines, l.placed[0].overflow), (11.0, 1, false));
    let e = extents(&l)[0];
    assert!(e[0] >= 50.0 && e[2] <= 250.0 && e[3] <= 60.0, "on the line, above it: {e:?}");
}

#[test]
fn a_long_address_shrinks_and_fits() {
    let address = "Hauptstraße 123a, 12345 Musterstadt-Nord, Hinterhaus";
    let l = fill::layout(&place("address", 1, address));
    let p = &l.placed[0];
    assert!(p.size < 11.0 && p.size >= 7.0 && p.lines == 1 && !p.overflow, "{p:?}");
    let e = extents(&l)[0];
    assert!(e[0] >= 50.0 && e[2] <= 250.0, "{e:?}");
    // longer still: two lines in the field below its label, then overflow
    let longer = "Hauptstraße 123a, Hinterhaus links, 3. Obergeschoss, 12345 Musterstadt-Nord";
    let l = fill::layout(&place("address", 2, longer));
    let p = &l.placed[0];
    assert!(p.lines == 2 && p.size >= 7.0 && !p.overflow, "{p:?}");
    for e in extents(&l) {
        assert!(e[0] >= 50.0 && e[2] <= 250.0 && e[1] >= 109.0 && e[3] <= 130.0, "no glyph outside: {e:?}");
    }
    let l = fill::layout(&place("address", 4, longer));
    assert!(l.placed[0].overflow && l.placed[0].size == 7.0, "{:?}", l.placed[0]);
}

#[test]
fn a_comb_takes_a_character_per_cell() {
    let l = fill::layout(&place("tax_id", 3, "12 345 678 901"));
    let marks: Vec<_> = l.marks.iter().map(|m| if let Mark::Text { x, text, .. } = m { (*x, text.clone()) } else { panic!() }).collect();
    assert_eq!(marks.len(), 11);
    let cell = 200.0 / 11.0;
    for (i, (x, t)) in marks.iter().enumerate() {
        let mid = x + helvetica::width(t, l.placed[0].size) / 2.0;
        assert!((mid - (50.0 + (i as f32 + 0.5) * cell)).abs() < 0.01, "{i}: {t} at {x}");
    }
    assert!(!l.placed[0].overflow);
    assert!(fill::layout(&place("tax_id", 3, "123456789012")).placed[0].overflow, "12 in 11 cells");
}

#[test]
fn a_box_takes_a_cross_and_a_drawn_box_snaps() {
    let mut r = place("agree", 5, "");
    r.template = serde_json::from_value(json!({"fields": [
        {"key": "agree", "type": "choice", "place": {"page": 1, "candidate": 5}},
        {"key": "city", "place": {"page": 1, "box2d": [50.0, 100.0, 70.0, 400.0]}}
    ]}))
    .unwrap();
    r.answers = [("agree".to_string(), json!(true)), ("city".to_string(), json!("Musterstadt"))].into();
    let l = fill::layout(&r);
    assert!(matches!(l.marks[0], Mark::Cross { b, .. } if b[0] > 300.0 && b[2] < 310.0));
    // 0–1000 of the page: 42–59 pt down, 60–238 across: the line at 60 pt
    assert_eq!(l.placed[1].candidate, Some(1));
    assert_eq!(l.color, [0x1a as f32 / 255.0; 3]);
}
