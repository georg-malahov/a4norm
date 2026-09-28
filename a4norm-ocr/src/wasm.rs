//! The browser's handle: the models in once, then a page at a time, as
//! pixels from a canvas or as an image file, out as A4Norm Forms'
//! `PageInspection` (without the geometry).
//!
//! In the threaded build the lines are read on the pool (`initThreadPool`,
//! `releaseThreadPool`, exported by src/pool.rs); without a pool, on the
//! caller's thread.

use crate::{pdf_jpeg, Ocr, Page};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = Ocr)]
pub struct WasmOcr(Ocr);

fn err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

fn work<R: Send>(f: impl FnOnce() -> R + Send) -> R {
    #[cfg(feature = "wasm-threads")]
    return crate::pool::run(f);
    #[cfg(not(feature = "wasm-threads"))]
    f()
}

#[wasm_bindgen(js_class = Ocr)]
impl WasmOcr {
    /// The detector's and the recognizer's `inference.onnx`, and the
    /// recognizer's `inference.yml` (its character list).
    #[wasm_bindgen(constructor)]
    pub fn new(det: &[u8], rec: &[u8], rec_yml: &str) -> Result<WasmOcr, JsError> {
        Ocr::new(det, rec, rec_yml).map(WasmOcr).map_err(err)
    }

    /// A page of RGBA pixels (a canvas's `ImageData`) that spans
    /// `width_pt` x `height_pt` points: `{sizePt, skewDeg, words: [{text,
    /// bbox, score}], printedSize, langs}`, boxes in points from the top left.
    pub fn inspect(&self, rgba: &[u8], width: u32, height: u32, width_pt: f32, height_pt: f32) -> Result<JsValue, JsError> {
        if rgba.len() != width as usize * height as usize * 4 {
            return Err(JsError::new("inspect: rgba is not width x height x 4 bytes"));
        }
        let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        let img = image::RgbImage::from_raw(width, height, rgb).unwrap();
        self.json(work(|| self.0.page(&img)).map_err(err)?, width_pt, height_pt)
    }

    /// The same for a JPEG or PNG file, or a PDF whose page is one JPEG (a
    /// scan, as A4Norm writes it).
    #[wasm_bindgen(js_name = inspectImage)]
    pub fn inspect_image(&self, bytes: &[u8], width_pt: f32, height_pt: f32) -> Result<JsValue, JsError> {
        let img = image::load_from_memory(pdf_jpeg(bytes).unwrap_or(bytes)).map_err(err)?.to_rgb8();
        self.json(work(|| self.0.page(&img)).map_err(err)?, width_pt, height_pt)
    }

    fn json(&self, page: Page, width_pt: f32, height_pt: f32) -> Result<JsValue, JsError> {
        js_sys::JSON::parse(&page.to_json([width_pt, height_pt])).map_err(|_| JsError::new("inspect: JSON"))
    }
}
