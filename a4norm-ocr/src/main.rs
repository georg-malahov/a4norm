//! a4norm-ocr MODELS_DIR PAGE [--det-long N] [--lines] [--mark MARKED.png]
//! a4norm-ocr --geometry PAGE [MARKED.png]
//! a4norm-ocr --form PAGE
//! a4norm-ocr --fill REQUEST.json OUT.pdf [IMAGE.png...] SOURCE.pdf|PAGE.jpg...
//!
//! MODELS_DIR holds det.onnx, rec.onnx and rec.yml (models.sh fetches them).
//! PAGE is an image, or a PDF whose page is one JPEG (a scan), of an A4
//! page. Prints the page's `PageInspection` JSON, or with --lines its text
//! lines; the timings go to stderr. --mark draws the candidates with their
//! numbers on the page; --geometry finds only them (no models needed);
//! --form says whether the page looks like a blank form (no models).

use a4norm_ocr::geometry::{self, Kind};
use a4norm_ocr::{pdf_jpeg, Ocr};
use image::{Rgb, RgbImage};
use std::time::Instant;

fn load(path: &str) -> RgbImage {
    let bytes = std::fs::read(path).expect("page");
    image::load_from_memory(pdf_jpeg(&bytes).unwrap_or(&bytes)).expect("an image").to_rgb8()
}

/// Points of an A4 page, portrait or landscape, in its pixels.
fn px_pt(img: &RgbImage) -> f32 {
    img.width() as f32 / if img.width() < img.height() { 595.28 } else { 841.89 }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--fill") {
        // REQUEST.json: {inspections, template, answers}; a PDF source, or
        // the JPEG pages of a scan
        let req: a4norm_ocr::fill::Request = serde_json::from_slice(&std::fs::read(&args[2]).expect("request")).expect("request JSON");
        let sources: Vec<Vec<u8>> = args[4..].iter().map(|f| std::fs::read(f).expect("source")).collect();
        let refs: Vec<&[u8]> = sources.iter().map(Vec::as_slice).collect();
        // the request's pictures (a signature), then the source
        let n = req.images.len();
        let (imgs, refs) = refs.split_at(n);
        let (layout, out) = if refs[0].starts_with(b"%PDF") {
            a4norm_ocr::pdf::fill_pdf(&req, refs[0], &refs[1..], imgs)
        } else {
            a4norm_ocr::pdf::fill_scan(&req, refs, imgs)
        }
        .expect("fill");
        std::fs::write(&args[3], &out.pdf).expect("out");
        println!("{}", layout.json());
        eprintln!("{} bytes{}", out.pdf.len(), if out.fallback { ", from the pictures" } else { "" });
        return;
    }
    if args.get(1).map(String::as_str) == Some("--form") {
        let img = load(&args[2]);
        let t = Instant::now();
        let f = geometry::looks_like_form(&img, px_pt(&img));
        eprintln!("looks like a form: {} ms", t.elapsed().as_millis());
        println!("{}", f.json());
        return;
    }
    if args.get(1).map(String::as_str) == Some("--geometry") {
        let img = load(&args[2]);
        let t = Instant::now();
        let g = geometry::find(&img, px_pt(&img), &[]);
        eprintln!("geometry {} ms, {} candidates", t.elapsed().as_millis(), g.candidates.len());
        println!("{{{}}}", g.json_fields(px_pt(&img)));
        if let Some(out) = args.get(3) {
            let mut m = mark(img.clone(), &g);
            if std::env::var_os("A4NORM_STROKES").is_some() {
                for (x0, y0, x1, y1, hz) in geometry::debug_strokes(&img, px_pt(&img)) {
                    let col = if hz { Rgb([200, 0, 200]) } else { Rgb([0, 170, 200]) };
                    let n = ((x1 - x0).abs().max((y1 - y0).abs())).max(1.0) as i64;
                    for i in 0..=n {
                        let t = i as f32 / n as f32;
                        dot(&mut m, (x0 + (x1 - x0) * t) as i64, (y0 + (y1 - y0) * t) as i64, 1, col);
                    }
                }
            }
            m.save(out).expect("marked page");
        }
        return;
    }
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
    let loaded = t0.elapsed();
    let img = load(path);
    let size = if img.width() < img.height() { [595.28, 841.89] } else { [841.89, 595.28] };
    let t1 = Instant::now();
    let (page, g) = ocr.inspect(&img, size).unwrap();
    let first = t1.elapsed();
    let t2 = Instant::now();
    ocr.inspect(&img, size).unwrap();
    let again = t2.elapsed();
    if args.iter().any(|a| a == "--lines") {
        for l in &page.lines {
            let b = l.bbox;
            println!("{:5.0} {:5.0} {:5.0} {:5.0}  h {:4.1}  {}", b[0], b[1], b[2], b[3], l.height, l.text());
        }
    } else {
        println!("{}", a4norm_ocr::inspection_json(&page, &g, size));
        if let Some(i) = args.iter().position(|a| a == "--mark") {
            // on the page the right way up, where the candidates are
            let upright = match page.orientation {
                90 => image::imageops::rotate90(&img),
                180 => image::imageops::rotate180(&img),
                270 => image::imageops::rotate270(&img),
                _ => img.clone(),
            };
            mark(upright, &g).save(&args[i + 1]).expect("marked page");
        }
    }
    eprintln!(
        "load {} ms · page {} ms first (plans included), {} ms again · turned {}° · {} lines, {} words, skew {:.2}°, \
         print {:.1} px, langs {:?}",
        loaded.as_millis(),
        first.as_millis(),
        again.as_millis(),
        page.orientation,
        page.lines.len(),
        page.lines.iter().map(|l| l.words.len()).sum::<usize>(),
        page.skew.to_degrees(),
        page.printed,
        page.langs
    );
}

/// The page, paler, with each candidate drawn and numbered: lines red,
/// fields blue, combs green with their cells, check boxes orange.
fn mark(mut img: RgbImage, g: &geometry::Geometry) -> RgbImage {
    for p in img.pixels_mut() {
        *p = Rgb(p.0.map(|v| 255 - (255 - v) / 3));
    }
    let s = (img.width() as f32 / 850.0).max(1.0).round() as i64;
    for c in &g.candidates {
        let b = c.bounds();
        let col = match c.kind {
            Kind::Line(_) => Rgb([220, 30, 30]),
            Kind::Rect(_) => Rgb([30, 80, 230]),
            Kind::Comb(..) => Rgb([20, 160, 60]),
            Kind::Box(_) => Rgb([240, 140, 0]),
        };
        match c.kind {
            Kind::Line(l) => {
                let n = (l.x1 - l.x0).max(1.0) as i64;
                for i in 0..=n {
                    let t = i as f32 / n as f32;
                    let (x, y) = (l.x0 + (l.x1 - l.x0) * t, l.y0 + (l.y1 - l.y0) * t);
                    dot(&mut img, x as i64, y as i64, s, col);
                }
            }
            Kind::Comb(r, n) => {
                rect(&mut img, &r, s, col);
                for k in 1..n {
                    let x = r[0] + (r[2] - r[0]) * k as f32 / n as f32;
                    for y in r[1] as i64..r[3] as i64 {
                        dot(&mut img, x as i64, y, s / 2 + 1, col);
                    }
                }
            }
            Kind::Rect(r) | Kind::Box(r) => rect(&mut img, &r, s, col),
        }
        number(&mut img, c.id, b[0] as i64, b[1] as i64 - 9 * s, s, col);
    }
    img
}

fn dot(img: &mut RgbImage, x: i64, y: i64, r: i64, col: Rgb<u8>) {
    for yy in y - r / 2..=y + r / 2 {
        for xx in x - r / 2..=x + r / 2 {
            if xx >= 0 && yy >= 0 && (xx as u32) < img.width() && (yy as u32) < img.height() {
                img.put_pixel(xx as u32, yy as u32, col);
            }
        }
    }
}

fn rect(img: &mut RgbImage, r: &[f32; 4], s: i64, col: Rgb<u8>) {
    let (x0, y0, x1, y1) = (r[0] as i64, r[1] as i64, r[2] as i64, r[3] as i64);
    for x in x0..=x1 {
        dot(img, x, y0, s, col);
        dot(img, x, y1, s, col);
    }
    for y in y0..=y1 {
        dot(img, x0, y, s, col);
        dot(img, x1, y, s, col);
    }
}

/// `n` in a 3 x 5 digit font, `s` pixels a dot, on a white patch.
fn number(img: &mut RgbImage, n: usize, x: i64, y: i64, s: i64, col: Rgb<u8>) {
    const D: [u16; 10] = [0x7b6f, 0x2c97, 0x73e7, 0x73cf, 0x5bc9, 0x79cf, 0x79ef, 0x7249, 0x7bef, 0x7bcf];
    let t = n.to_string();
    let (w, h) = ((t.len() as i64 * 4 + 1) * s, 7 * s);
    for yy in 0..h {
        for xx in 0..w {
            dot(img, x + xx, y + yy, 1, Rgb([255, 255, 255]));
        }
    }
    for (k, ch) in t.bytes().enumerate() {
        let bits = D[(ch - b'0') as usize];
        for r in 0..5 {
            for c in 0..3 {
                if bits >> (14 - (r * 3 + c)) & 1 == 1 {
                    let (px, py) = (x + (1 + k as i64 * 4 + c) * s, y + (1 + r) * s);
                    for yy in 0..s {
                        for xx in 0..s {
                            dot(img, px + xx, py + yy, 1, col);
                        }
                    }
                }
            }
        }
    }
}
