//! The browser's API of the filling (`web` feature): the same in this
//! crate's own module, for the free form mode, and in the OCR module.
//!
//! ```js
//! const layout = fillLayout(JSON.stringify({ inspections, template, answers }));
//! const r = fillPdf(JSON.stringify(request), pdfBytes, pagePictures, imageBytes);
//! const r = fillScan(JSON.stringify(request), pageJpegs, imageBytes);
//! // r: {pdf: Uint8Array, fallback, baseSize, placed}
//! // several PDFs and scanned pages as one, the PDFs' pages as they are
//! const pdf = mergePdf([{ pdf: a }, { jpg, dpi: 200 }, { pdf: b, pages: [2, 1] }], { title });
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

/// Several PDFs and scanned pages as one PDF, the PDFs' pages as they are
/// (`src/merge.rs`): `parts` are, in order, `{ pdf, pages? }` (pages from 1,
/// chosen and ordered; all when absent) and `{ jpg, dpi?, turn?, widthMm?,
/// heightMm? }` (a page of its own size, or its pixels at `dpi`, or A4;
/// turned quarters clockwise). `opts.title` goes into the PDF's /Info.
#[wasm_bindgen(js_name = mergePdf)]
pub fn merge_pdf(parts: js_sys::Array, opts: Option<JsValue>) -> Result<js_sys::Uint8Array, JsError> {
    let get = |o: &JsValue, k: &str| js_sys::Reflect::get(o, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED);
    let num = |v: JsValue| v.as_f64().map(|x| x as f32);
    let mut bytes: Vec<(Vec<u8>, JsValue)> = vec![];
    for p in parts.iter() {
        let pdf = get(&p, "pdf");
        let src = if pdf.is_undefined() || pdf.is_null() { get(&p, "jpg") } else { pdf };
        if src.is_undefined() || src.is_null() {
            return Err(JsError::new(&format!("part {}: neither pdf nor jpg", bytes.len() + 1)));
        }
        bytes.push((js_sys::Uint8Array::new(&src).to_vec(), p));
    }
    let list: Vec<crate::merge::Part> = bytes
        .iter()
        .map(|(b, p)| {
            if get(p, "pdf").is_undefined() || get(p, "pdf").is_null() {
                let size = match (num(get(p, "widthMm")), num(get(p, "heightMm"))) {
                    (Some(w), Some(h)) => Some([w, h]),
                    _ => None,
                };
                crate::merge::Part::Jpeg { jpg: b, dpi: num(get(p, "dpi")), turn: num(get(p, "turn")).unwrap_or(0.0) as u8, size_mm: size }
            } else {
                let pages = get(p, "pages");
                let pages = js_sys::Array::is_array(&pages).then(|| js_sys::Array::from(&pages).iter().filter_map(|v| v.as_f64()).map(|v| v as u32).collect());
                crate::merge::Part::Pdf { pdf: b, pages }
            }
        })
        .collect();
    let title = opts.as_ref().and_then(|o| get(o, "title").as_string());
    let out = crate::merge::merge(&list, title.as_deref()).map_err(|e| JsError::new(&e))?;
    Ok(js_sys::Uint8Array::from(out.as_slice()))
}
