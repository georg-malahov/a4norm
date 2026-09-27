//! The browser's API (wasm-bindgen): one call per run of the command line.
//!
//! ```js
//! await init({ module_or_path: '.../a4norm_bg.wasm' });
//! await initThreadPool(navigator.hardwareConcurrency);   // threaded build only
//! const r = process([{ bytes, name }], ['--format', 'jpg', '--dpi', '200'],
//!                   (stage, done) => ...);                // done: 0..1, rising
//! // r = { pages: [{ jpg: Uint8Array, dpi }], report: "photo-0.jpg\n  page 1:\n    - ..." }
//! const pdf = pack(jpgs, dpis, gray);
//!
//! // Edit panel: fill round spots from their surroundings
//! const jpg2 = inpaint(jpg, new Float32Array([x, y, r, ...]), 88);   // page px
//! // size control: every page written again, in parallel
//! const small = recompress(jpgs, { dpi: 150, quality: 60, gray: false });
//! // small = [{ jpg: Uint8Array, dpi }, ...]
//! // the Edit panel's rotate: quarter turns clockwise
//! const turned = rotate(jpg, 1, 88);
//!
//! // camera preview: the document in a video frame drawn to a ~600 px canvas
//! const { kind, quads } = detect(ctx.getImageData(0, 0, w, h).data, w, h);
//! ```

use crate::{encode_page, io, parse_args, run, Source};
use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

#[cfg(feature = "wasm-threads")]
pub use wasm_bindgen_rayon::init_thread_pool;

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
        sources.push(Source { name, rasters: vec![src] });
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
    let pages = run(sources, &o, &mut |l| {
        report.push_str(l);
        report.push('\n');
    }, &tick)
    .map_err(|e| JsValue::from_str(&e.0))?;
    let out = Array::new();
    for p in &pages {
        let page = Object::new();
        set(&page, "jpg", &Uint8Array::from(encode_page(p, &o).as_slice()));
        set(&page, "dpi", &JsValue::from_f64(p.dpi as f64));
        out.push(&page);
    }
    tick("done", 1.0);
    let r = Object::new();
    set(&r, "pages", &out);
    set(&r, "report", &JsValue::from_str(&report));
    Ok(r)
}

/// JPEG pages -> one PDF. `gray`: each page made single-channel first
/// (quality 88), as the page's black-and-white option asks.
#[wasm_bindgen]
pub fn pack(jpgs: Array, dpis: Vec<u32>, gray: bool) -> Result<Uint8Array, JsValue> {
    let mut pages = vec![];
    for (i, j) in jpgs.iter().enumerate() {
        let bytes = Uint8Array::new(&j).to_vec();
        if gray {
            let src = io::decode(&bytes, "page").map_err(|e| JsValue::from_str(&e.0))?;
            let g = crate::img::Img::from_planes(vec![crate::img::gray_resized(&src, src.w, src.h)]);
            pages.push(io::encode_jpeg(&g, 88, false, dpis[i] as usize));
        } else {
            pages.push(bytes);
        }
    }
    let d: Vec<usize> = dpis.iter().map(|&x| x as usize).collect();
    Ok(Uint8Array::from(io::write_pdf(&pages, &d).as_slice()))
}

/// A finished page with round spots filled from what surrounds them, the
/// smart eraser. `dots`: x, y, r in page pixels, three numbers per spot.
/// The page is written again at `quality` (default: the pages' own), its dpi
/// kept; a grey page stays grey.
#[wasm_bindgen]
pub fn inpaint(jpg: Vec<u8>, dots: Vec<f32>, quality: Option<u8>) -> Result<Uint8Array, JsValue> {
    let spots: Vec<(f64, f64, f64)> = dots.chunks_exact(3).map(|d| (d[0] as f64, d[1] as f64, d[2] as f64)).collect();
    let q = quality.unwrap_or(crate::Opts::default().quality);
    let out = crate::edit::inpaint_jpeg(&jpg, &spots, q).map_err(|e| JsValue::from_str(&e.0))?;
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
    let out = Array::new();
    for group in pages.chunks(4) {
        let done = crate::ops::par_map(group.len(), |i| crate::edit::recompress(&group[i], dpi, quality, gray));
        for r in done {
            let (jpg, d) = r.map_err(|e| JsValue::from_str(&e.0))?;
            let page = Object::new();
            set(&page, "jpg", &Uint8Array::from(jpg.as_slice()));
            set(&page, "dpi", &JsValue::from_f64(d as f64));
            out.push(&page);
        }
    }
    Ok(out)
}

/// A finished page turned by `quarter_turns` quarter turns clockwise (1 =
/// 90°, 2 = 180°, 3 = 270°), written again at `quality` (default: the pages'
/// own) with its dpi kept; a grey page stays grey.
#[wasm_bindgen]
pub fn rotate(jpg: Vec<u8>, quarter_turns: i32, quality: Option<u8>) -> Result<Uint8Array, JsValue> {
    let q = quality.unwrap_or(crate::Opts::default().quality);
    let out = crate::edit::rotate(&jpg, quarter_turns, q).map_err(|e| JsValue::from_str(&e.0))?;
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
    let o = crate::Opts::default();
    let mut report = vec![];
    let (kind, quads): (&str, Vec<crate::detect::Quad>) = match crate::page::locate(&src, &o, &mut report) {
        Ok(crate::page::Found::Sheet(q, why)) => (if why.ends_with(crate::detect::RECEIPT) { "receipt" } else { "sheet" }, vec![q]),
        Ok(crate::page::Found::Cards(qs, _)) => ("cards", qs),
        Ok(crate::page::Found::Spread(s)) => ("spread", s.quads.to_vec()),
        _ => ("none", vec![]),
    };
    let flat: Vec<f32> = quads.iter().flat_map(|q| q.iter().flat_map(|p| [p.0 as f32, p.1 as f32])).collect();
    let out = Object::new();
    set(&out, "kind", &JsValue::from_str(kind));
    set(&out, "quads", &js_sys::Float32Array::from(flat.as_slice()).into());
    Ok(out)
}
