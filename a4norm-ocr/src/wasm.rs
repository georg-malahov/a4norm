//! The browser's handle: the models in once, then a page at a time, as
//! pixels from a canvas or as an image file, out as A4Norm Forms'
//! `PageInspection`. `formGeometry` finds the geometry alone, without the
//! models.
//!
//! In the threaded build the lines are read on the pool (`initThreadPool`,
//! `releaseThreadPool`, exported by src/pool.rs); without a pool, on the
//! caller's thread.

use crate::{geometry, inspection_json, pdf_jpeg, px_pt, Ocr};
use image::RgbImage;
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
    /// bbox, score}], printedSize, langs, lines, rects, combs, boxes,
    /// typicalFieldHeight}`, in points from the top left.
    pub fn inspect(&self, rgba: &[u8], width: u32, height: u32, width_pt: f32, height_pt: f32) -> Result<JsValue, JsError> {
        self.inspect_page(rgb(rgba, width, height)?, [width_pt, height_pt])
    }

    /// The same for a JPEG or PNG file, or a PDF whose page is one JPEG (a
    /// scan, as A4Norm writes it).
    #[wasm_bindgen(js_name = inspectImage)]
    pub fn inspect_image(&self, bytes: &[u8], width_pt: f32, height_pt: f32) -> Result<JsValue, JsError> {
        self.inspect_page(decode(bytes)?, [width_pt, height_pt])
    }

    fn inspect_page(&self, img: RgbImage, size: [f32; 2]) -> Result<JsValue, JsError> {
        let (page, g) = work(|| self.0.inspect(&img, size)).map_err(err)?;
        parse(&inspection_json(&page, &g, size))
    }
}

/// The geometry of a page of RGBA pixels, without reading it (no models):
/// `{sizePt, lines, rects, combs, boxes, typicalFieldHeight}`. Letters of
/// large print can pass for small fields here; `Ocr.inspect` knows the
/// words and leaves them out.
#[wasm_bindgen(js_name = formGeometry)]
pub fn form_geometry(rgba: &[u8], width: u32, height: u32, width_pt: f32, height_pt: f32) -> Result<JsValue, JsError> {
    geometry_json(rgb(rgba, width, height)?, [width_pt, height_pt])
}

/// The same for an image file or a one-JPEG PDF.
#[wasm_bindgen(js_name = formGeometryImage)]
pub fn form_geometry_image(bytes: &[u8], width_pt: f32, height_pt: f32) -> Result<JsValue, JsError> {
    geometry_json(decode(bytes)?, [width_pt, height_pt])
}

fn geometry_json(img: RgbImage, size: [f32; 2]) -> Result<JsValue, JsError> {
    let s = px_pt(img.width(), size);
    let g = geometry::find(&img, s, &[]);
    parse(&format!("{{\"sizePt\":[{:.2},{:.2}],{}}}", size[0], size[1], g.json_fields(s)))
}

fn rgb(rgba: &[u8], width: u32, height: u32) -> Result<RgbImage, JsError> {
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(JsError::new("rgba is not width x height x 4 bytes"));
    }
    let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    Ok(RgbImage::from_raw(width, height, rgb).unwrap())
}

fn decode(bytes: &[u8]) -> Result<RgbImage, JsError> {
    Ok(image::load_from_memory(pdf_jpeg(bytes).unwrap_or(bytes)).map_err(err)?.to_rgb8())
}

fn parse(json: &str) -> Result<JsValue, JsError> {
    js_sys::JSON::parse(json).map_err(|_| JsError::new("JSON"))
}
