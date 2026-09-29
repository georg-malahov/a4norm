//! Where the answers go (fill.rs): the demo form's 12 values at their
//! printed places on its three pictures, and the rules of the sizes.

use a4norm_ocr::fill::{self, Inspection, Mark, Request, Template};
use a4norm_ocr::{font, inspection_json, pdf_jpeg, Ocr};
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
    Request { inspections: vec![ins], template, answers, color: None, min_size: None, images: vec![], texts: vec![] }
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
        images: vec![],
        texts: vec![],
    }
}

/// Every text mark's extent: `[x0, top, x1, foot]`.
fn extents(l: &fill::Layout) -> Vec<[f32; 4]> {
    l.marks
        .iter()
        .filter_map(|m| match m {
            Mark::Text { x, y, size, text, .. } => {
                Some([*x, y - font::CAP * size, x + font::width(text, *size), y + font::DESCENT * size])
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
        let mid = x + font::width(t, l.placed[0].size) / 2.0;
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

#[test]
fn a_value_runs_on_over_its_candidates() {
    // two lines of 100 pt: the words that fit the first at the base size,
    // the rest on the second, in the order of their numbers
    let mut ins = page();
    ins.lines = serde_json::from_value(json!([
        {"id": 1, "x0": 50.0, "y0": 60.0, "x1": 150.0, "y1": 60.0},
        {"id": 6, "x0": 50.0, "y0": 80.0, "x1": 150.0, "y1": 80.0}
    ]))
    .unwrap();
    let r = Request {
        inspections: vec![ins],
        template: serde_json::from_value(json!({"fields": [{"key": "address", "place": {"page": 1, "candidates": [6, 1]}}]})).unwrap(),
        answers: [("address".to_string(), json!("Musterweg 12, Hinterhaus, 12345 Musterstadt"))].into(),
        color: None,
        min_size: None,
        images: vec![],
        texts: vec![],
    };
    let l = fill::layout(&r);
    let rows: Vec<(f32, String)> = l.marks.iter().map(|m| if let Mark::Text { y, text, .. } = m { (*y, text.clone()) } else { panic!() }).collect();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].0 < 60.0 && rows[1].0 < 80.0 && rows[1].0 > 60.0, "{rows:?}");
    assert_eq!(format!("{} {}", rows[0].1, rows[1].1), "Musterweg 12, Hinterhaus, 12345 Musterstadt");
    assert!(font::width(&rows[0].1, l.base) <= 100.0 - 3.0);
    assert_eq!((l.placed[0].candidate, l.placed[0].lines), (Some(1), 2));
}

#[test]
fn sizes_come_in_steps() {
    // six fields under labels of slightly different heights, as on KG 1:
    // each would take its own size (12, 11.8, … 10.4); on the ladder, and
    // brought down to the step most of them take, they come to two sizes,
    // the lower only where the higher does not fit
    let mut ins = page();
    ins.printed_size = 10.0;
    ins.typical_field_height = 25.0;
    let mut rects = vec![];
    let mut words = vec![];
    let mut fields = vec![];
    let mut answers = std::collections::HashMap::new();
    for i in 0..6 {
        let y = 200.0 + i as f32 * 40.0;
        // the label's foot a little lower each time: 10.4 to 12.9 pt left
        let foot = y + 12.0 + 0.5 * i as f32;
        rects.push(json!({"id": 10 + i, "box": [50.0, y, 250.0, y + 25.0]}));
        words.push(json!({"text": "Familienname", "bbox": [52.0, y + 2.0, 110.0, foot]}));
        fields.push(json!({"key": format!("f{i}"), "place": {"page": 1, "candidate": 10 + i}}));
        answers.insert(format!("f{i}"), json!("Musterfrau"));
    }
    ins.rects = serde_json::from_value(json!(rects)).unwrap();
    ins.words = serde_json::from_value(json!(words)).unwrap();
    let r = Request {
        inspections: vec![ins],
        template: serde_json::from_value(json!({"fields": fields})).unwrap(),
        answers,
        color: None,
        min_size: None,
        images: vec![],
        texts: vec![],
    };
    let l = fill::layout(&r);
    assert_eq!(l.base, 12.0);
    let all: Vec<f32> = l.placed.iter().map(|p| p.size).collect();
    let sizes: std::collections::BTreeSet<u32> = all.iter().map(|s| (s * 10.0).round() as u32).collect();
    assert_eq!(sizes.len(), 2, "{all:?}");
    assert!(all.iter().all(|s| s.fract() == 0.0 && *s < 12.0), "steps of the ladder below 12: {all:?}");
    assert!(all.iter().filter(|&&s| s == 11.0).count() >= 4, "most at the common step: {all:?}");
}

#[test]
fn a_value_moved_and_set_larger_by_hand() {
    // the value as placed, then 4 pt right and 3 pt up at 16 pt: the size
    // exactly (above the base, larger than the room), the text moved as a
    // whole
    let plain = fill::layout(&place("name", 1, "Greenholt"));
    let mut r = place("name", 1, "Greenholt");
    r.template = serde_json::from_value(json!({"fields": [
        {"key": "name", "place": {"page": 1, "candidate": 1}, "size": 16.0, "shift": [4.0, -3.0]}
    ]}))
    .unwrap();
    let l = fill::layout(&r);
    let (a, b) = (&plain.placed[0], &l.placed[0]);
    assert_eq!((b.size, b.lines, b.overflow), (16.0, 1, false), "{b:?}");
    assert!((b.x - a.x - 4.0).abs() < 1e-3, "{a:?} {b:?}");
    let Mark::Text { x, y, size, .. } = &l.marks[0] else { panic!() };
    assert_eq!((*x, *y, *size), (b.x, b.y, 16.0));
    // its foot still clears the line, 3 pt higher
    assert!(extents(&l)[0][3] <= 60.0 - 3.0, "{:?}", extents(&l));
    // too long for the line at 16 pt: two lines, still 16 pt
    let mut r = place("address", 1, "Hauptstraße 123a, 12345 Musterstadt-Nord");
    r.template = serde_json::from_value(json!({"fields": [{"key": "address", "place": {"page": 1, "candidate": 1}, "size": 16.0}]})).unwrap();
    let p = &fill::layout(&r).placed[0];
    assert_eq!((p.size, p.lines), (16.0, 2), "{p:?}");
}

#[test]
fn a_size_by_hand_stays_out_of_the_page_size() {
    // three values that shrink to 8 pt bring the page's fourth down with
    // them; one set by hand keeps its size and brings none down
    let mut ins = page();
    ins.lines = serde_json::from_value(json!([
        {"id": 1, "x0": 50.0, "y0": 60.0, "x1": 110.0, "y1": 60.0},
        {"id": 6, "x0": 50.0, "y0": 90.0, "x1": 110.0, "y1": 90.0},
        {"id": 7, "x0": 50.0, "y0": 120.0, "x1": 110.0, "y1": 120.0},
        {"id": 8, "x0": 150.0, "y0": 60.0, "x1": 450.0, "y1": 60.0}
    ]))
    .unwrap();
    let long = "Musterstadt-Nord";
    let fields = |hand: serde_json::Value| {
        json!({"fields": [
            {"key": "a", "place": {"page": 1, "candidate": 1}},
            {"key": "b", "place": {"page": 1, "candidate": 6}},
            {"key": "c", "place": {"page": 1, "candidate": 7}},
            {"key": "d", "place": {"page": 1, "candidate": 8}, "size": hand}
        ]})
    };
    let mut r = Request {
        inspections: vec![ins],
        template: serde_json::from_value(fields(json!(null))).unwrap(),
        answers: [("a", long), ("b", long), ("c", long), ("d", "Kurz")].map(|(k, v)| (k.to_string(), json!(v))).into(),
        color: None,
        min_size: None,
        images: vec![],
        texts: vec![],
    };
    let chosen = fill::layout(&r);
    let small = chosen.placed[0].size;
    assert!(small < chosen.base && chosen.placed[3].size == small, "{:?}", chosen.placed);
    r.template = serde_json::from_value(fields(json!(13.0))).unwrap();
    let l = fill::layout(&r);
    assert_eq!(l.placed[3].size, 13.0);
    assert_eq!(l.placed[..3].iter().map(|p| p.size).collect::<Vec<_>>(), [small; 3]);
}

#[test]
fn a_comb_and_a_cross_moved_by_hand() {
    let mut r = place("tax_id", 3, "12 345 678 901");
    let plain = fill::layout(&r);
    r.template = serde_json::from_value(json!({"fields": [
        {"key": "tax_id", "place": {"page": 1, "candidate": 3}, "shift": [1.5, 2.0]},
        {"key": "agree", "type": "choice", "place": {"page": 1, "candidate": 5}, "shift": [-1.0, 0.5]}
    ]}))
    .unwrap();
    r.answers.insert("agree".to_string(), json!(true));
    let l = fill::layout(&r);
    let cells = |l: &fill::Layout| -> Vec<(f32, f32)> {
        l.marks.iter().filter_map(|m| if let Mark::Text { x, y, .. } = m { Some((*x, *y)) } else { None }).collect()
    };
    let (a, b) = (cells(&plain), cells(&l));
    assert_eq!(a.len(), 11);
    assert!(a.iter().zip(&b).all(|(p, q)| (q.0 - p.0 - 1.5).abs() < 1e-3 && (q.1 - p.1 - 2.0).abs() < 1e-3), "{a:?} {b:?}");
    let cross = l.marks.iter().find_map(|m| if let Mark::Cross { b, .. } = m { Some(*b) } else { None }).unwrap();
    let side = 10.0 * (1.0 - 2.0 * 0.18);
    assert!((cross[0] - (300.0 + 1.8 - 1.0)).abs() < 1e-3 && (cross[1] - (150.0 + 1.8 + 0.5)).abs() < 1e-3, "{cross:?}");
    assert!((cross[2] - cross[0] - side).abs() < 1e-3);
    let p = l.placed.iter().find(|p| p.key == "agree").unwrap();
    assert_eq!((p.x, p.y), (304.0, 155.5));
}

#[test]
fn free_text_where_it_was_put() {
    let mut r = place("name", 1, "Greenholt");
    r.texts = serde_json::from_value(json!([
        {"page": 1, "x": 320.0, "y": 200.0, "size": 12.0, "text": "Straße ş ł"},
        {"x": 50.0, "y": 300.0, "size": 9.0, "text": "  "}
    ]))
    .unwrap();
    let l = fill::layout(&r);
    assert_eq!(l.placed.len(), 1, "free text is no answer");
    let free: Vec<&Mark> = l.marks.iter().filter(|m| matches!(m, Mark::Text { x, .. } if *x == 320.0)).collect();
    assert_eq!(free, [&Mark::Text { page: 1, x: 320.0, y: 200.0, size: 12.0, angle: 0.0, text: "Straße ş ł".into() }]);
    assert_eq!(l.marks.len(), 2, "blank text is left out");
}
