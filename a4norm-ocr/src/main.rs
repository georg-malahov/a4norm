//! a4norm-ocr MODELS_DIR PAGE [--det-long N] [--lines]
//!
//! MODELS_DIR holds det.onnx, rec.onnx and rec.yml (models.sh fetches them). PAGE is an image, or a
//! PDF whose page is one JPEG (a scan). Prints the page's `PageInspection`
//! JSON for an A4 page, or with --lines its text lines; the timings go to
//! stderr.

use a4norm_ocr::{pdf_jpeg, Ocr};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: a4norm-ocr MODELS_DIR PAGE [--det-long N] [--lines]");
        std::process::exit(2);
    }
    let (dir, path) = (&args[1], &args[2]);
    let read = |f: &str| std::fs::read(format!("{dir}/{f}")).unwrap_or_else(|e| panic!("{dir}/{f}: {e}"));
    let t0 = Instant::now();
    let mut ocr = Ocr::new(&read("det.onnx"), &read("rec.onnx"), &String::from_utf8(read("rec.yml")).unwrap())
        .expect("models");
    if let Some(i) = args.iter().position(|a| a == "--det-long") {
        ocr.det_long = args[i + 1].parse().expect("--det-long N");
    }
    let load = t0.elapsed();
    let bytes = std::fs::read(path).expect("page");
    let img = image::load_from_memory(pdf_jpeg(&bytes).unwrap_or(&bytes)).expect("an image").to_rgb8();
    let t1 = Instant::now();
    let page = ocr.page(&img).unwrap();
    let first = t1.elapsed();
    let t2 = Instant::now();
    ocr.page(&img).unwrap();
    let again = t2.elapsed();
    if args.iter().any(|a| a == "--lines") {
        for l in &page.lines {
            let b = l.bbox;
            println!("{:5.0} {:5.0} {:5.0} {:5.0}  h {:4.1}  {}", b[0], b[1], b[2], b[3], l.height, l.text());
        }
    } else {
        println!("{}", page.to_json([595.28, 841.89]));
    }
    eprintln!(
        "load {} ms · page {} ms first (plans included), {} ms again · {} lines, {} words, skew {:.2}°, \
         print {:.1} px, langs {:?}",
        load.as_millis(),
        first.as_millis(),
        again.as_millis(),
        page.lines.len(),
        page.lines.iter().map(|l| l.words.len()).sum::<usize>(),
        page.skew.to_degrees(),
        page.printed,
        page.langs
    );
}
