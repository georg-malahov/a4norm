//! A form filled in, without reading it: where each answer goes and how
//! large (`fill`), and the filled form as a PDF over its own pages or a
//! scan's pictures (`pdf`), in Arimo (`font`). No models: the pages'
//! candidates come from `a4norm-geometry` (the scanner's `formGeometry`)
//! or from the OCR module's `inspect`.
//!
//! `merge` puts several PDFs and scanned pages into one, the PDFs' pages
//! as they are.
//!
//! In the browser this is a module of its own (`web/build.sh`), for the
//! free form mode: the scanner's module and this one, nothing else. The
//! OCR module carries the same API, from this crate.

pub mod fill;
pub mod font;
pub mod merge;
pub mod pdf;

#[cfg(all(target_arch = "wasm32", feature = "web"))]
mod wasm;
