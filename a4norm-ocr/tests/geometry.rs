//! The demo form's geometry from its three pictures, and the fields of
//! three official forms. No models needed; the vector PDFs are rendered by
//! poppler's pdftoppm, and a test that needs it or an official form passes
//! with a note when either is missing.

use a4norm_ocr::geometry::{self, Candidate, Kind};
use a4norm_ocr::pdf_jpeg;
use image::RgbImage;
use std::path::PathBuf;
use std::process::Command;

const FORMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/forms/");

fn picture(name: &str) -> RgbImage {
    let bytes = std::fs::read(format!("{FORMS}{name}")).unwrap();
    image::load_from_memory(pdf_jpeg(&bytes).unwrap_or(&bytes)).unwrap().to_rgb8()
}

/// A PDF's page at 200 dpi, or None without pdftoppm or the file. Each call
/// renders to a file of its own: tests running at once may ask for the
/// same page.
fn render(pdf: &str, page: u32) -> Option<RgbImage> {
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = format!("{FORMS}{pdf}");
    if !std::path::Path::new(&path).exists() {
        return None;
    }
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let out = std::env::temp_dir().join(format!("a4norm-geometry-{}-{page}-{n}", pdf.replace(['/', '.'], "-")));
    let p = page.to_string();
    let ok = Command::new("pdftoppm")
        .args(["-r", "200", "-png", "-f", &p, "-l", &p, "-singlefile", &path])
        .arg(&out)
        .status()
        .is_ok_and(|s| s.success());
    ok.then(|| image::open(out.with_extension("png")).unwrap().to_rgb8())
}

fn find(img: &RgbImage) -> Vec<Candidate> {
    geometry::find(img, img.width() as f32 / 595.28, &[]).candidates
}

fn count(c: &[Candidate]) -> (usize, usize, usize, usize) {
    let n = |f: fn(&Kind) -> bool| c.iter().filter(|c| f(&c.kind)).count();
    (
        n(|k| matches!(k, Kind::Line(_))),
        n(|k| matches!(k, Kind::Rect(_))),
        n(|k| matches!(k, Kind::Comb(..))),
        n(|k| matches!(k, Kind::Box(_))),
    )
}

/// The candidates' ends in points, by number.
fn ends(c: &[Candidate], px_pt: f32) -> Vec<(usize, [f32; 4])> {
    c.iter()
        .map(|c| {
            let b = match c.kind {
                Kind::Line(l) => [l.x0, l.y0, l.x1, l.y1],
                _ => c.bounds(),
            };
            (c.id, b.map(|v| v / px_pt))
        })
        .collect()
}

/// The largest distance between the same candidate's ends on two pictures
/// of one page, once one picture is laid on the other: turned and scaled by
/// the best fit (a scan is tilted, a printout shrunk).
fn apart(a: &[(usize, [f32; 4])], b: &[(usize, [f32; 4])]) -> f32 {
    let pts = |v: &[(usize, [f32; 4])]| -> Vec<(f64, f64)> {
        v.iter().flat_map(|(_, e)| [(e[0] as f64, e[1] as f64), (e[2] as f64, e[3] as f64)]).collect()
    };
    let (p, q) = (pts(a), pts(b));
    // q = s p + t, s and t complex: least squares
    let n = p.len() as f64;
    let (mp, mq) = (
        p.iter().fold((0.0, 0.0), |s, v| (s.0 + v.0 / n, s.1 + v.1 / n)),
        q.iter().fold((0.0, 0.0), |s, v| (s.0 + v.0 / n, s.1 + v.1 / n)),
    );
    let (mut re, mut im, mut den) = (0.0, 0.0, 0.0);
    for (u, v) in p.iter().zip(&q) {
        let (ux, uy, vx, vy) = (u.0 - mp.0, u.1 - mp.1, v.0 - mq.0, v.1 - mq.1);
        re += ux * vx + uy * vy;
        im += ux * vy - uy * vx;
        den += ux * ux + uy * uy;
    }
    let (sr, si) = (re / den, im / den);
    p.iter()
        .zip(&q)
        .map(|(u, v)| {
            let (ux, uy) = (u.0 - mp.0, u.1 - mp.1);
            let (x, y) = (sr * ux - si * uy + mq.0, si * ux + sr * uy + mq.1);
            ((x - v.0).powi(2) + (y - v.1).powi(2)).sqrt() as f32
        })
        .fold(0.0, f32::max)
}

#[test]
fn the_demo_form_three_ways() {
    let scan = picture("demo-blank-scan.jpg");
    let real = picture("demo-blank-a4norm-scan.pdf");
    let mut pictures = vec![("demo-blank-scan.jpg", scan), ("demo-blank-a4norm-scan.pdf", real)];
    match render("demo-blank.pdf", 1) {
        Some(img) => pictures.insert(0, ("demo-blank.pdf", img)),
        None => eprintln!("demo-blank.pdf skipped: no pdftoppm"),
    }
    let mut first: Option<(Vec<Candidate>, f32)> = None;
    for (name, img) in &pictures {
        let px_pt = img.width() as f32 / 595.28;
        let c = find(img);
        // 20 writing lines (14 dotted, 6 solid) and the 4 boxes of the two
        // yes/no questions
        assert_eq!(count(&c), (20, 0, 0, 4), "{name}");
        match &first {
            None => first = Some((c, px_pt)),
            Some((f, fpx)) => {
                let same: Vec<_> = f.iter().zip(&c).map(|(a, b)| (a.id, std::mem::discriminant(&a.kind)) == (b.id, std::mem::discriminant(&b.kind))).collect();
                assert!(same.iter().all(|&s| s), "{name}: the numbering differs");
                let d = apart(&ends(f, *fpx), &ends(&c, px_pt));
                eprintln!("{name}: ends within {d:.1} pt of {}'s", pictures[0].0);
                assert!(d <= 6.0, "{name}: {d:.1} pt apart");
            }
        }
    }
}

#[test]
fn the_demo_forms_boxes_and_first_lines() {
    let img = picture("demo-blank-scan.jpg");
    let px_pt = img.width() as f32 / 595.28;
    let c = find(&img);
    // "Name: ......" and "Vorname: ......" open the page, left to right
    let Kind::Line(name) = c[0].kind else { panic!("{:?}", c[0]) };
    let Kind::Line(first) = c[1].kind else { panic!("{:?}", c[1]) };
    assert!((name.x0 / px_pt - 73.0).abs() < 5.0 && (name.y0 / px_pt - 106.0).abs() < 5.0, "{name:?}");
    assert!(first.x0 > name.x1, "{first:?}");
    // the "Ja" and "Nein" boxes are 20 pt squares; inside, a little less
    for b in c.iter().filter_map(|c| if let Kind::Box(b) = c.kind { Some(b) } else { None }) {
        let side = (b[2] - b[0]) / px_pt;
        assert!((16.0..=20.0).contains(&side), "{b:?}");
    }
}

#[test]
fn official_forms() {
    // (form, page, lines at least, fields, combs' cells, boxes)
    let cases: [(&str, u32, usize, usize, &[usize], usize); 3] = [
        // Familienkasse: fields open on top, the tax ID in groups, IBAN, BIC
        ("official/ba-kg1-kindergeld.pdf", 2, 3, 20, &[2, 3, 3, 3, 2, 3, 3, 3, 34, 11], 9),
        // Jobcenter: rounded fields, square and round boxes (and its note)
        ("official/ba-jobcenter-hauptantrag.pdf", 1, 5, 15, &[], 6),
        // Frankfurt: dotted lines, and boxes that are the character "□"
        ("official/frankfurt-aufenthaltstitel.pdf", 1, 17, 0, &[], 9),
    ];
    for (pdf, page, lines, rects, cells, boxes) in cases {
        let Some(img) = render(pdf, page) else {
            eprintln!("{pdf} skipped: no form here (examples/forms/official.py) or no pdftoppm");
            continue;
        };
        let c = find(&img);
        let (l, r, _, b) = count(&c);
        let combs: Vec<usize> = c.iter().filter_map(|c| if let Kind::Comb(_, n) = c.kind { Some(n) } else { None }).collect();
        eprintln!("{pdf} p. {page}: {l} lines, {r} fields, combs {combs:?}, {b} boxes");
        assert!(l >= lines, "{pdf}: {l} lines");
        assert_eq!((r, b), (rects, boxes), "{pdf}");
        let mut got = combs.clone();
        let mut want = cells.to_vec();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(got, want, "{pdf}");
    }
}

/// The scanner, if it was built (`cargo build --release` in a4norm-rs/).
fn scanner() -> Option<PathBuf> {
    let p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../a4norm-rs/target/release/a4norm"));
    p.exists().then_some(p)
}

#[test]
fn the_demo_scans_after_the_scanner() {
    // A4Norm Forms reads a photo of paper after the scanner has made a page
    // of it (cropped, levelled, cleaned, 200 dpi JPEG): its rules come out
    // bolder, and a figure's loop must not pass for a box
    let Some(a4norm) = scanner() else {
        return eprintln!("skipped: no scanner built (cargo build --release in a4norm-rs/)");
    };
    for scan in ["demo-blank-a4norm-scan.pdf", "demo-filled-scan.jpg"] {
        for look in ["auto", "magic", "color"] {
            let out = std::env::temp_dir().join(format!("a4norm-geometry-pipe-{}-{look}.jpg", scan.replace('.', "-")));
            let ok = Command::new(&a4norm)
                .args(["--format", "jpg", "--dpi", "200", "--look", look, "-o"])
                .arg(&out)
                .arg(format!("{FORMS}{scan}"))
                .output()
                .is_ok_and(|o| o.status.success());
            assert!(ok, "{scan} {look}: the scanner failed");
            let img = image::open(&out).unwrap().to_rgb8();
            let px_pt = img.width() as f32 / 595.28;
            let boxes: Vec<[f32; 4]> = find(&img).iter().filter_map(|c| if let Kind::Box(b) = c.kind { Some(b) } else { None }).collect();
            let sides: Vec<f32> = boxes.iter().map(|b| (b[2] - b[0]) / px_pt).collect();
            eprintln!("{scan} {look}: boxes {sides:.1?}");
            assert_eq!(boxes.len(), 4, "{scan} {look}: {sides:?}");
            assert!(sides.iter().all(|s| (16.0..=21.0).contains(s)), "{scan} {look}: {sides:?}");
        }
    }
}

#[test]
fn a_blank_form_looks_like_one() {
    // D24: a page is offered for filling in when 6 or more of its
    // candidates are empty, and 60 % of them at least. The blank demo in
    // its three pictures and KG 1 are; the filled demo (10 of 21 empty) and
    // a page of KG 1's notes (nothing to fill in) are not.
    let offer = |f: geometry::FormLook| f.empty >= 6 && f.empty as f32 >= 0.6 * f.total as f32;
    let look = |img: &RgbImage| geometry::looks_like_form(img, img.width() as f32 / 595.28);
    for name in ["demo-blank-scan.jpg", "demo-blank-a4norm-scan.pdf"] {
        let f = look(&picture(name));
        eprintln!("{name}: {f:?}");
        assert!(offer(f) && f.empty >= 19, "{name}: {f:?}");
    }
    let f = look(&picture("demo-filled-scan.jpg"));
    eprintln!("demo-filled-scan.jpg: {f:?}");
    assert!(!offer(f) && (9..=11).contains(&f.empty) && (20..=22).contains(&f.total), "{f:?}");
    for (pdf, page, form) in [("demo-blank.pdf", 1, true), ("official/ba-kg1-kindergeld.pdf", 2, true), ("official/ba-kg1-kindergeld.pdf", 4, false)] {
        let Some(img) = render(pdf, page) else {
            eprintln!("skipped {pdf}: no pdftoppm or no form here");
            continue;
        };
        let f = look(&img);
        eprintln!("{pdf} p. {page}: {f:?}");
        assert_eq!(offer(f), form, "{pdf} p. {page}: {f:?}");
    }
}

#[test]
fn bold_boxes_are_boxes() {
    // Three bold boxes before their lines, as a letter printed on A5 has
    // them once the scanner has made an A4 page of it: 16 pt inside, 2.9 pt
    // of border. A small square with as bold a border (a letter's counter)
    // is none.
    let px = 200.0 / 72.0;
    let mut img = RgbImage::from_pixel((595.28 * px) as u32, (841.89 * px) as u32, image::Rgb([255, 255, 255]));
    let mut square = |x: f32, y: f32, inside: f32, border: f32| {
        let (x0, y0, x1, y1) = ((x * px) as u32, (y * px) as u32, ((x + inside + 2.0 * border) * px) as u32, ((y + inside + 2.0 * border) * px) as u32);
        let b = (border * px).round() as u32;
        for yy in y0..y1 {
            for xx in x0..x1 {
                if xx < x0 + b || xx >= x1 - b || yy < y0 + b || yy >= y1 - b {
                    img.put_pixel(xx, yy, image::Rgb([20, 20, 20]));
                }
            }
        }
    };
    for i in 0..3 {
        square(60.0, 200.0 + 50.0 * i as f32, 16.0, 2.9);
    }
    square(300.0, 200.0, 7.0, 2.9);
    let boxes: Vec<[f32; 4]> = find(&img).iter().filter_map(|c| if let Kind::Box(b) = c.kind { Some(b.map(|v| v / px)) } else { None }).collect();
    assert_eq!(boxes.len(), 3, "{boxes:?}");
    assert!(boxes.iter().all(|b| b[0] < 70.0 && (b[2] - b[0] - 16.0).abs() < 1.5), "{boxes:?}");
}
