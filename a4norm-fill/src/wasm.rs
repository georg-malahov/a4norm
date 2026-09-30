//! The browser's API of the filling (`web` feature): the same in this
//! crate's own module, for the free form mode, and in the OCR module.
//!
//! ```js
//! const layout = fillLayout(JSON.stringify({ inspections, template, answers }));
//! const r = fillPdf(JSON.stringify(request), pdfBytes, pagePictures, imageBytes);
//! const r = fillScan(JSON.stringify(request), pageJpegs, imageBytes);
//! // r: {pdf: Uint8Array, fallback, baseSize, placed}
//! ```

use wasm_bindgen::prelude::*;

fn err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

fn parse(json: &str) -> Result<JsValue, JsError> {
    js_sys::JSON::parse(json).map_err(|_| JsError::new("JSON"))
}

/// Where each answer goes and its size, without drawing it (no models):
/// `request` is `{inspections, template, answers, color?, minSize?,
/// images?, texts?}` as JSON (`src/fill.rs`; a field may carry `size` and
/// `shift`, set by hand); the result `{baseSize, placed: [{key, page, x, y, size,
/// lines, overflow, kind, candidate?, lost?}]}`, in points.
#[wasm_bindgen(js_name = fillLayout)]
pub fn fill_layout(request: &str) -> Result<JsValue, JsError> {
    let req: crate::fill::Request = serde_json::from_str(request).map_err(err)?;
    parse(&crate::fill::layout(&req).json())
}

/// The filled form from a PDF: a new PDF of its own pages with the answers
/// over them (`src/pdf.rs`). `pictures` (JPEG, the pages as they were
/// inspected) stand in for pages that cannot be read; may be empty.
/// `imageBytes` (PNG or JPEG, optional) are the pictures the request's
/// `images` place over the pages, in their order: a signature, a stamp.
/// `{pdf, fallback, baseSize, placed}`.
#[wasm_bindgen(js_name = fillPdf)]
pub fn fill_pdf(request: &str, source: &[u8], pictures: js_sys::Array, image_bytes: Option<js_sys::Array>) -> Result<JsValue, JsError> {
    let req: crate::fill::Request = serde_json::from_str(request).map_err(err)?;
    let (pics, imgs) = (bytes_list(&pictures), image_bytes.as_ref().map(bytes_list).unwrap_or_default());
    let refs: Vec<&[u8]> = pics.iter().map(Vec::as_slice).collect();
    let img_refs: Vec<&[u8]> = imgs.iter().map(Vec::as_slice).collect();
    let (layout, out) = crate::pdf::fill_pdf(&req, source, &refs, &img_refs).map_err(|e| JsError::new(&e))?;
    filled(&layout, out)
}

/// The filled form from scanned pages (JPEG): each page its picture, and
/// the answers over it; `imageBytes` as for `fillPdf`.
/// `{pdf, fallback: false, baseSize, placed}`.
#[wasm_bindgen(js_name = fillScan)]
pub fn fill_scan(request: &str, pages: js_sys::Array, image_bytes: Option<js_sys::Array>) -> Result<JsValue, JsError> {
    let req: crate::fill::Request = serde_json::from_str(request).map_err(err)?;
    let (pics, imgs) = (bytes_list(&pages), image_bytes.as_ref().map(bytes_list).unwrap_or_default());
    let refs: Vec<&[u8]> = pics.iter().map(Vec::as_slice).collect();
    let img_refs: Vec<&[u8]> = imgs.iter().map(Vec::as_slice).collect();
    let (layout, out) = crate::pdf::fill_scan(&req, &refs, &img_refs).map_err(|e| JsError::new(&e))?;
    filled(&layout, out)
}

fn bytes_list(a: &js_sys::Array) -> Vec<Vec<u8>> {
    a.iter().map(|v| js_sys::Uint8Array::new(&v).to_vec()).collect()
}

fn filled(layout: &crate::fill::Layout, out: crate::pdf::Output) -> Result<JsValue, JsError> {
    let r = parse(&layout.json())?;
    let set = |k: &str, v: &JsValue| js_sys::Reflect::set(&r, &JsValue::from_str(k), v).map(|_| ()).map_err(|_| JsError::new("result"));
    set("pdf", &js_sys::Uint8Array::from(out.pdf.as_slice()).into())?;
    set("fallback", &JsValue::from_bool(out.fallback))?;
    Ok(r)
}
