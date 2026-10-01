//! The browser's API (wasm-bindgen): one call per run of the command line.
//!
//! ```js
//! await init({ module_or_path: '.../a4norm_bg.wasm' });
//! await initThreadPool(navigator.hardwareConcurrency);   // threaded build only
//! const r = process([{ bytes, name }], ['--format', 'jpg', '--dpi', '200'],
//!                   (stage, done) => ...);                // done: 0..1, rising
//! // r = { pages: [{ jpg: Uint8Array, dpi, geom }], photos: [...],
//! //       report: "photo-0.jpg\n  page 1:\n    - ..." }
//! // a file may carry its own look and corners set by hand (docs/api.md):
//! //   { bytes, name, look: 'magic', quad: Float64Array, kind: 'spread' }
//! // and where its document goes on the page, in mm (docs/api.md, size):
//! //   { bytes, name, place: { w: 125, x: 42.5, y: 15 } }  or  place: { size: 'real' }
//! // each page: { jpg, dpi, geom, size, placed, content, sheet }
//! const pdf = pack(jpgs, dpis, gray);
//! // pages turned without touching their JPEGs: quarter turns clockwise each
//! const turnedPdf = pack(jpgs, dpis, gray, new Int32Array([0, 1, 0, 2]));
//! // threaded build, once idle: the threads leave and their workers close;
//! // initThreadPool starts them again (docs/threads.md)
//! await releaseThreadPool();
//!
//! // Edit panel: fill round spots from their surroundings
//! const jpg2 = inpaint(jpg, new Float32Array([x, y, r, ...]), 88);   // page px
//! // size control: every page written again, in parallel
//! const small = recompress(jpgs, { dpi: 150, quality: 60, gray: false });
//! // small = [{ jpg: Uint8Array, dpi }, ...]
//! // the Edit panel's rotate: quarter turns clockwise
//! const turned = rotate(jpg, 1, 88);
//!
//! // the tray: a photo's thumbnail, EXIF applied, a JPEG decoded at 1/2..1/8
//! const thumb = thumbnail(bytes, 320, 80);
//! // camera preview: the document in a video frame drawn to a ~600 px canvas
//! const { kind, quads } = detect(ctx.getImageData(0, 0, w, h).data, w, h);
//!
//! // after process: does a page look like a blank form? (offer "Fill in")
//! const { empty, total } = looksLikeForm(r.pages[0]);   // { jpg, dpi }
//! // its fields, to fill it in by hand (the fill module writes the PDF)
//! const { lines, rects, combs, boxes } = formGeometry(r.pages[0]);
//! ```

use crate::{encode_page, io, parse_args, run, Source};
use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

#[cfg(feature = "wasm-threads")]
pub use crate::pool::{init_thread_pool, release_thread_pool};

/// `f` on the thread pool in the threaded build (src/pool.rs), here in the
/// other. `on` gets `f`'s progress on this thread, the one JS runs on.
fn work<R: Send>(f: impl FnOnce(&dyn Fn(&str, f64)) -> R + Send, on: impl Fn(&str, f64)) -> R {
    #[cfg(feature = "wasm-threads")]
    return crate::pool::run(f, on);
    #[cfg(not(feature = "wasm-threads"))]
    f(&on)
}

fn get(o: &JsValue, k: &str) -> JsValue {
    Reflect::get(o, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED)
}

fn set(o: &Object, k: &str, v: &JsValue) {
    Reflect::set(o, &JsValue::from_str(k), v).unwrap();
}

/// Photos -> pages, with the same flags and the same report as the command
/// line. Throws the message the command line would exit with.
#[wasm_bindgen]
pub fn process(files: Array, args: Array, progress: Option<Function>) -> Result<Object, JsValue> {
    let mut argv: Vec<String> = args.iter().filter_map(|a| a.as_string()).collect();
    let mut sources = vec![];
    let mut names = vec![];
    for f in files.iter() {
        let name = get(&f, "name").as_string().unwrap_or_else(|| "photo.jpg".into());
        let bytes = Uint8Array::new(&get(&f, "bytes")).to_vec();
        let src = io::decode(&bytes, &name).map_err(|e| JsValue::from_str(&e.0))?;
        drop(bytes);
        names.push(name.clone());
        let mut s = Source::new(name, vec![src]);
        s.look = get(&f, "look").as_string();
        if let Some(l) = &s.look {
            if !["auto", "magic", "color", "original"].contains(&l.as_str()) {
                return Err(JsValue::from_str(&format!("a4norm: look '{}': auto, magic, color or original", l)));
            }
        }
        let quad = get(&f, "quad");
        if !quad.is_undefined() && !quad.is_null() {
            let v: Vec<f64> = Array::from(&quad).iter().filter_map(|x| x.as_f64()).collect();
            let kind = get(&f, "kind").as_string();
            s.hand = Some(crate::page::Hand::new(&v, kind.as_deref()).map_err(|e| JsValue::from_str(&format!("a4norm: {}: quad: {}", s.name, e)))?);
        }
        // place: { w, x?, y? } in mm, or { size: "real" | "fit" } (size.rs)
        let place = get(&f, "place");
        if !place.is_undefined() && !place.is_null() {
            let mm = |k: &str| get(&place, k).as_f64();
            s.place = Some(match (get(&place, "size").as_string().as_deref(), mm("w")) {
                (Some("real"), _) => crate::size::Place::Real,
                (Some("fit"), _) => crate::size::Place::Fit,
                (_, Some(w)) if w > 0.0 => crate::size::Place::At { w, x: mm("x"), y: mm("y") },
                _ => return Err(JsValue::from_str(&format!("a4norm: {}: place: {{ w, x?, y? }} in mm or {{ size: \"real\" | \"fit\" }}", s.name))),
            });
        }
        sources.push(s);
    }
    argv.extend(names.iter().cloned());
    let o = parse_args(&argv).map_err(|e| JsValue::from_str(&e.0))?;
    let mut report = String::new();
    report.push_str(&names.join(", "));
    report.push('\n');
    let tick = |stage: &str, done: f64| {
        if let Some(p) = &progress {
            let _ = p.call2(&JsValue::NULL, &JsValue::from_str(stage), &JsValue::from_f64(done));
        }
    };
    let (done, jpgs) = work(|tick| {
        let done = run(sources, &o, &mut |l| {
            report.push_str(l);
            report.push('\n');
        }, tick)?;
        let jpgs: Vec<Vec<u8>> = done.pages.iter().map(|p| encode_page(p, &o)).collect();
        Ok::<_, crate::Fail>((done, jpgs))
    }, tick)
    .map_err(|e| JsValue::from_str(&e.0))?;
    let num = |v: usize| JsValue::from_f64(v as f64);
    let out = Array::new();
    for (p, jpg) in done.pages.iter().zip(&jpgs) {
        let page = Object::new();
        set(&page, "jpg", &Uint8Array::from(jpg.as_slice()));
        set(&page, "dpi", &num(p.dpi));
        let geom = Object::new();
        set(&geom, "sources", &p.sources.iter().map(|&s| num(s)).collect::<Array>());
        set(&geom, "key", &JsValue::from_str(&p.geo.key));
        set(&geom, "look", &JsValue::from_str(p.geo.look));
        set(&geom, "flat", &JsValue::from_str(p.geo.flat));
        set(&geom, "lines", &JsValue::from_bool(p.geo.lines));
        set(&geom, "width", &num(p.img.w));
        set(&geom, "height", &num(p.img.h));
        set(&page, "geom", &geom);
        // size, placed, content, sheet (size.rs): the document's size and
        // where it lies on the page
        let fields = js_sys::JSON::parse(&format!("{{{}}}", p.layout.json_fields()))?;
        for k in ["size", "placed", "content", "sheet"] {
            set(&page, k, &get(&fields, k));
        }
        out.push(&page);
    }
    let photos = Array::new();
    for (s, pno) in &done.photos {
        let ph = Object::new();
        let flat: Vec<f64> = s.quads.iter().flat_map(|q| q.iter().flat_map(|p| [p.0, p.1])).collect();
        set(&ph, "kind", &JsValue::from_str(s.kind));
        // f64: sent back as they came, the corners give the same key
        set(&ph, "quads", &js_sys::Float64Array::from(flat.as_slice()).into());
        set(&ph, "width", &num(s.w));
        set(&ph, "height", &num(s.h));
        set(&ph, "page", &num(*pno));
        set(&ph, "hand", &JsValue::from_bool(s.hand));
        photos.push(&ph);
    }
    tick("done", 1.0);
    let r = Object::new();
    set(&r, "pages", &out);
    set(&r, "photos", &photos);
    set(&r, "report", &JsValue::from_str(&report));
    Ok(r)
}

/// JPEG pages -> one PDF. `gray`: each page made single-channel first
/// (quality 88), as the page's black-and-white option asks. `turns`: quarter
/// turns clockwise per page (0-3, missing ones 0); the JPEG goes in as it is,
/// the page's sides swap and the image is drawn turned, so a turn costs
/// nothing and loses nothing.
#[wasm_bindgen]
pub fn pack(jpgs: Array, dpis: Vec<u32>, gray: bool, turns: Option<Vec<i32>>) -> Result<Uint8Array, JsValue> {
    let jpgs: Vec<Vec<u8>> = jpgs.iter().map(|j| Uint8Array::new(&j).to_vec()).collect();
    let pdf = work(|_| {
        let mut pages = vec![];
        for (i, bytes) in jpgs.into_iter().enumerate() {
            if gray {
                let src = io::decode(&bytes, "page")?;
                let g = crate::img::Img::from_planes(vec![crate::img::gray_resized(&src, src.w, src.h)]);
                pages.push(io::encode_jpeg(&g, 88, false, dpis[i] as usize));
            } else {
                pages.push(bytes);
            }
        }
        let d: Vec<usize> = dpis.iter().map(|&x| x as usize).collect();
        Ok::<_, crate::Fail>(io::write_pdf_turned(&pages, &d, &turns.unwrap_or_default()))
    }, |_, _| {})
    .map_err(|e| JsValue::from_str(&e.0))?;
    Ok(Uint8Array::from(pdf.as_slice()))
}

/// A finished page with round spots filled from what surrounds them, the
/// smart eraser. `dots`: x, y, r in page pixels, three numbers per spot.
/// The page is written again at `quality` (default: the pages' own), its dpi
/// kept; a grey page stays grey.
#[wasm_bindgen]
pub fn inpaint(jpg: Vec<u8>, dots: Vec<f32>, quality: Option<u8>) -> Result<Uint8Array, JsValue> {
    let spots: Vec<(f64, f64, f64)> = dots.chunks_exact(3).map(|d| (d[0] as f64, d[1] as f64, d[2] as f64)).collect();
    let q = quality.unwrap_or(crate::Opts::default().quality);
    let out = work(|_| crate::edit::inpaint_jpeg(&jpg, &spots, q), |_, _| {}).map_err(|e| JsValue::from_str(&e.0))?;
    Ok(Uint8Array::from(out.as_slice()))
}

/// Pages written again smaller, for the size control. `opts`: { dpi,
/// quality, gray }; a page is brought down to `dpi` when it is higher, never
/// up. Pages go four at a time, in parallel in the threaded build: more at
/// once would hold too many decoded pages for a phone's memory.
#[wasm_bindgen]
pub fn recompress(jpgs: Array, opts: JsValue) -> Result<Array, JsValue> {
    let num = |k: &str, d: f64| get(&opts, k).as_f64().unwrap_or(d);
    let dpi = num("dpi", 200.0) as usize;
    let quality = num("quality", crate::Opts::default().quality as f64).clamp(1.0, 100.0) as u8;
    let gray = get(&opts, "gray").as_bool().unwrap_or(false);
    let pages: Vec<Vec<u8>> = jpgs.iter().map(|j| Uint8Array::new(&j).to_vec()).collect();
    let done = work(|_| {
        let mut done = vec![];
        for group in pages.chunks(4) {
            done.extend(crate::ops::par_map(group.len(), |i| crate::edit::recompress(&group[i], dpi, quality, gray)));
        }
        done
    }, |_, _| {});
    let out = Array::new();
    for r in done {
        let (jpg, d) = r.map_err(|e| JsValue::from_str(&e.0))?;
        let page = Object::new();
        set(&page, "jpg", &Uint8Array::from(jpg.as_slice()));
        set(&page, "dpi", &JsValue::from_f64(d as f64));
        out.push(&page);
    }
    Ok(out)
}

/// A finished page turned by `quarter_turns` quarter turns clockwise (1 =
/// 90°, 2 = 180°, 3 = 270°), written again at `quality` (default: the pages'
/// own) with its dpi kept; a grey page stays grey.
#[wasm_bindgen]
pub fn rotate(jpg: Vec<u8>, quarter_turns: i32, quality: Option<u8>) -> Result<Uint8Array, JsValue> {
    let q = quality.unwrap_or(crate::Opts::default().quality);
    let out = work(|_| crate::edit::rotate(&jpg, quarter_turns, q), |_, _| {}).map_err(|e| JsValue::from_str(&e.0))?;
    Ok(Uint8Array::from(out.as_slice()))
}

/// Camera preview: what a scan of this video frame would take, found the same
/// way. `rgba` is a canvas's ImageData, `w`×`h`; about 600 px on the long side
/// is enough, the finder works at that size. Returns `{ kind, quads }`:
/// `kind` is "sheet", "receipt", "cards", "spread" or "none", and `quads`
/// holds 8 numbers per quad, its corners clockwise from the top left in the
/// frame's pixels (a spread: its two pages; cards: one quad each).
#[wasm_bindgen]
pub fn detect(rgba: &[u8], w: u32, h: u32) -> Result<Object, JsValue> {
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return Err(JsValue::from_str("detect: rgba must hold w*h*4 bytes"));
    }
    let px: Vec<u8> = rgba[..w * h * 4].chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let src = crate::img::Src { w, h, px };
    let (kind, quads): (&str, Vec<crate::detect::Quad>) = work(|_| {
        let o = crate::Opts::default();
        let mut report = vec![];
        match crate::page::locate(&src, &o, &mut report) {
            Ok(crate::page::Found::Sheet(q, why)) => (if why.ends_with(crate::detect::RECEIPT) { "receipt" } else { "sheet" }, vec![q]),
            Ok(crate::page::Found::Cards(qs, _)) => ("cards", qs),
            Ok(crate::page::Found::Spread(s)) => ("spread", s.quads.to_vec()),
            _ => ("none", vec![]),
        }
    }, |_, _| {});
    let flat: Vec<f32> = quads.iter().flat_map(|q| q.iter().flat_map(|p| [p.0 as f32, p.1 as f32])).collect();
    let out = Object::new();
    set(&out, "kind", &JsValue::from_str(kind));
    set(&out, "quads", &js_sys::Float32Array::from(flat.as_slice()).into());
    Ok(out)
}

/// A photo's thumbnail as a JPEG, `max_side` px on its long side at most,
/// turned by its EXIF. A JPEG is decoded straight at 1/2, 1/4 or 1/8 of its
/// size; PNG and WebP decode whole. `quality` defaults to 80.
#[wasm_bindgen]
pub fn thumbnail(bytes: &[u8], max_side: u32, quality: Option<u8>) -> Result<Uint8Array, JsValue> {
    let out = work(|_| crate::edit::thumbnail(bytes, max_side as usize, quality.unwrap_or(80)), |_, _| {}).map_err(|e| JsValue::from_str(&e.0))?;
    Ok(Uint8Array::from(out.as_slice()))
}

/// Whether one of `process`'s pages, `{ jpg, dpi }`, looks like a blank form,
/// for offering to fill it in (D24): `{ empty, total, lines, rects, combs,
/// boxes }`, the page's writing lines (under 400 pt), fields, combs and check
/// boxes, and how many of them nothing is written in (a4norm-geometry). The
/// site offers it when `empty >= 6 && empty >= 0.6 * total`.
#[wasm_bindgen(js_name = looksLikeForm)]
pub fn looks_like_form(page: JsValue) -> Result<Object, JsValue> {
    let jpg = Uint8Array::new(&get(&page, "jpg")).to_vec();
    let dpi = get(&page, "dpi").as_f64().unwrap_or(200.0);
    let src = io::decode(&jpg, "page").map_err(|e| JsValue::from_str(&e.0))?;
    let img = image::RgbImage::from_raw(src.w as u32, src.h as u32, src.px).ok_or_else(|| JsValue::from_str("a4norm: page"))?;
    let f = a4norm_geometry::looks_like_form(&img, dpi as f32 / 72.0);
    let out = Object::new();
    for (k, v) in [("empty", f.empty), ("total", f.total), ("lines", f.lines), ("rects", f.rects), ("combs", f.combs), ("boxes", f.boxes)] {
        set(&out, k, &JsValue::from_f64(v as f64));
    }
    Ok(out)
}

/// The candidates of one of `process`'s pages, `{ jpg, dpi, sizePt? }`, for
/// filling it in by hand (the free form mode, with the fill module):
/// `{ sizePt, lines, rects, combs, boxes, typicalFieldHeight }`, each
/// candidate with its `id` in reading order and `empty` (nothing written in
/// it), a field with `label` where its printed label ends (the fill module
/// sets a value below it), in points from the page's top left; the same candidates and ids as
/// the OCR module's `formGeometry` gives (a4norm-geometry). `sizePt` is the
/// page's: A4 by default when the page is A4-shaped, as the scanner's are,
/// else its pixels at `dpi`.
#[wasm_bindgen(js_name = formGeometry)]
pub fn form_geometry(page: JsValue) -> Result<JsValue, JsValue> {
    let jpg = Uint8Array::new(&get(&page, "jpg")).to_vec();
    let dpi = get(&page, "dpi").as_f64().unwrap_or(200.0) as f32;
    let src = io::decode(&jpg, "page").map_err(|e| JsValue::from_str(&e.0))?;
    let img = image::RgbImage::from_raw(src.w as u32, src.h as u32, src.px).ok_or_else(|| JsValue::from_str("a4norm: page"))?;
    let (w, h) = (img.width() as f32, img.height() as f32);
    let given: Vec<f32> = match get(&page, "sizePt") {
        v if Array::is_array(&v) => Array::from(&v).iter().filter_map(|v| v.as_f64()).map(|v| v as f32).collect(),
        _ => vec![],
    };
    let size = match given[..] {
        [sw, sh] => [sw, sh],
        _ if ((w.max(h) / w.min(h)) / 2f32.sqrt() - 1.0).abs() < 0.01 => {
            if w < h { [595.28, 841.89] } else { [841.89, 595.28] }
        }
        _ => [w * 72.0 / dpi, h * 72.0 / dpi],
    };
    let px_pt = w / size[0];
    let g = a4norm_geometry::find(&img, px_pt, &[]);
    let empty = a4norm_geometry::empty(&img, px_pt, &g);
    let labels = a4norm_geometry::label_feet(&img, px_pt, &g);
    let json = format!("{{\"sizePt\":[{:.2},{:.2}],{}}}", size[0], size[1], g.json_fields_empty(px_pt, &empty, &labels));
    js_sys::JSON::parse(&json)
}
