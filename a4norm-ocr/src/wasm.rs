//! The browser's handle: models in, a page (RGBA from a canvas) in, JSON out.

use crate::Ocr;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = Ocr)]
pub struct WasmOcr(Ocr);

#[wasm_bindgen(js_class = Ocr)]
impl WasmOcr {
    #[wasm_bindgen(constructor)]
    pub fn new(det: &[u8], rec: &[u8], dict: &str) -> Result<WasmOcr, JsError> {
        Ocr::new(det, rec, dict).map(WasmOcr).map_err(|e| JsError::new(&e.to_string()))
    }

    /// `{"skew": radians, "lines": [{"text", "score", "bbox": [x0,y0,x1,y1]}]}`
    pub fn page(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<String, JsError> {
        let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        let img = image::RgbImage::from_raw(width, height, rgb).ok_or_else(|| JsError::new("size"))?;
        let (lines, skew) = self.0.page(&img).map_err(|e| JsError::new(&e.to_string()))?;
        let mut s = format!("{{\"skew\":{skew},\"lines\":[");
        for (i, l) in lines.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "{{\"text\":{:?},\"score\":{:.3},\"bbox\":[{},{},{},{}]}}",
                l.text, l.score, l.bbox[0], l.bbox[1], l.bbox[2], l.bbox[3]
            ));
        }
        s.push_str("]}");
        Ok(s)
    }
}
