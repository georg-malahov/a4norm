//! The browser's API (wasm-bindgen): one call per run of the command line.
//!
//! ```js
//! await init({ module_or_path: '.../a4norm_bg.wasm' });
//! await initThreadPool(navigator.hardwareConcurrency);   // threaded build only
//! const r = process([{ bytes, name }], ['--format', 'jpg', '--dpi', '200'],
//!                   (stage, done) => ...);                // done: 0..1, rising
//! // r = { pages: [{ jpg: Uint8Array, dpi }], report: "photo-0.jpg\n  page 1:\n    - ..." }
//! const pdf = pack(jpgs, dpis, gray);
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
