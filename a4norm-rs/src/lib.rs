//! a4norm's page finishing in Rust -- the stage 1 experiment of the port.
//! The same crate builds the native command line and the WebAssembly module
//! the browser would load.

// Index loops read as the pixel arithmetic they are.
#![allow(clippy::needless_range_loop)]

pub mod finish;
pub mod ops;

pub use finish::{finish, Params, Rgb};

// ---- the WebAssembly surface: plain exports over linear memory, so the
// benchmark can time each stage from JavaScript.

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use std::alloc::{alloc as a, dealloc as d, Layout};

    static mut PAGE: Option<Rgb> = None;

    #[no_mangle]
    pub extern "C" fn alloc(n: usize) -> *mut u8 {
        unsafe { a(Layout::from_size_align(n, 16).unwrap()) }
    }

    #[no_mangle]
    pub extern "C" fn dealloc(p: *mut u8, n: usize) {
        unsafe { d(p, Layout::from_size_align(n, 16).unwrap()) }
    }

    #[allow(static_mut_refs)]
    fn page() -> &'static mut Rgb {
        unsafe { PAGE.as_mut().unwrap() }
    }

    /// Load an RGB8 page.
    #[no_mangle]
    pub extern "C" fn load(px: *const u8, w: usize, h: usize) {
        let s = unsafe { std::slice::from_raw_parts(px, w * h * 3) };
        unsafe { PAGE = Some(Rgb::from_rgb8(s, w, h)) };
    }

    #[no_mangle]
    pub extern "C" fn flat() {
        finish::flat_field(page(), &Params::default());
    }

    #[no_mangle]
    pub extern "C" fn neutral() -> f64 {
        finish::neutralize_ink(page(), &Params::default())
    }

    #[no_mangle]
    pub extern "C" fn tone() {
        finish::tone(page(), &Params::default());
    }

    /// Write the page back as RGB8.
    #[no_mangle]
    pub extern "C" fn store(px: *mut u8) {
        let p = page();
        let s = unsafe { std::slice::from_raw_parts_mut(px, p.w * p.h * 3) };
        p.to_rgb8(s);
    }
}
