//! a4norm-ocr MODELS_DIR PAGE [DET_LONG]
//!
//! MODELS_DIR holds det.onnx, rec.onnx and dict.txt. Prints one JSON line per
//! text line, then the timings on stderr.

use a4norm_ocr::Ocr;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, page) = (&args[1], &args[2]);
    let read = |f: &str| std::fs::read(format!("{dir}/{f}")).expect(f);
    let t0 = Instant::now();
    let mut ocr = Ocr::new(&read("det.onnx"), &read("rec.onnx"), &String::from_utf8(read("dict.txt")).unwrap())
        .expect("models");
    if let Some(l) = args.get(3) {
        ocr.det_long = l.parse().unwrap();
    }
    let load = t0.elapsed();
    let img = image::open(page).expect("page").to_rgb8();
    let t1 = Instant::now();
    let (boxes, _) = ocr.detect(&img).unwrap();
    let det = t1.elapsed();
    let t2 = Instant::now();
    let (lines, skew) = ocr.page(&img).unwrap();
    let all = t2.elapsed();
    for l in &lines {
        println!(
            "{{\"text\":{:?},\"score\":{:.3},\"bbox\":[{},{},{},{}]}}",
            l.text, l.score, l.bbox[0], l.bbox[1], l.bbox[2], l.bbox[3]
        );
    }
    eprintln!(
        "load {:?} · detect {:?} ({} boxes, first run incl. plan) · page {:?} ({} lines, skew {:.2}°)",
        load, det, boxes.len(), all, lines.len(), skew.to_degrees()
    );
}
