# a4norm-rs — a4norm in Rust

The a4norm pipeline, ported from the Python script at the repository root.
One crate builds three things:

- **`a4norm`**, the command line. It takes the same flags, prints the same
  report line by line and writes the same PDF as the script. The Docker images
  (`:latest`, `:full`) ship it.
- **`web/dist/st`**, WebAssembly on one thread, for any browser.
- **`web/dist/mt`**, WebAssembly on a rayon pool over Web Workers, for a page
  served cross-origin isolated (COOP `same-origin` + COEP `require-corp`).

There is no ImageMagick inside and no Python.
- Photos decode in Rust: JPEG, PNG and WebP, with the EXIF turn applied.
- HEIC goes through whatever converter the host has (`magick`, `heif-convert`).
  In the browser it goes through libheif, as before.
- PDF input goes through poppler, as the script does.
- `:full` keeps calling the `a4norm-seg` helper when brightness finds no sheet.

## How it follows the script

Each operator the script asked ImageMagick for is reproduced with
ImageMagick's own geometry. Most of these are in `src/ops.rs`:

- resize weights, axis order and default filter;
- `-sample` offsets;
- the Gaussian's width and its sub-sampled taps;
- the Octagon, Disk, Diamond and Square kernels;
- Canny, including its hysteresis quirks;
- the Hough accumulator and its MVG rounding;
- the Radon deskew;
- EWA perspective and affine resampling;
- the `-trim` box;
- threshold percentages taken of `QuantumRange + 1`.

The script's 8-bit hand-off between stages is kept as well. Each stage rounds
its page to 8 bits, as each intermediate file did.

Two things are knowingly not reproduced exactly:
- **JPEG decoding.** `jpeg-decoder` lands within ±3 levels of libjpeg-turbo.
- **`-blur` wider than the page.** The colour copy's light estimate is taken
  over blocks, with the replicated edge pixels exact (60 dB against ImageMagick).

The goal is pages that cannot be told apart by eye or in print, not
bit-identity.

`python3 tests/snapshot.py compare a4norm-rs/target/release/a4norm` runs every
public example under 14 flag sets through both implementations. It shows the
report lines side by side and the PSNR of each page. At the time of the port:
- 61 of 98 reports are identical line for line;
- in the rest, a percentage differs in its last digit, or a JPEG decode moves
  one borderline decision;
- pages match at 53–61 dB.

## Speed and memory

All on an Apple M4 Max, `--dpi 200`.

| | script (native IM) | script in the browser (Pyodide + magick.wasm) | Rust native, 1 thread | wasm `st` | wasm `mt` (14 threads) |
|---|---|---|---|---|---|
| examples/landing-invoice | 3.9 s | 7.8 s | 0.4 s | 0.59 s | 0.29 s |
| examples/notebook-photo | 4 s | 9.9 s (21 MP) | 0.5 s | 0.75 s | 0.32 s |
| specimen card, front + back | 3.8 s | 4.3 s | 0.2 s | 0.30 s | 0.17 s |
| a 12 MP phone photo | 11–15 s | ~12 s | 1.7 s | 1.65 s | 0.51 s |

Memory is the peak of live data, counted with `A4MEM=1`.

| | at 200 dpi | at 300 dpi |
|---|---|---|
| 12 MP photo | 113 MB | 222 MB |
| 50 MP photo | 240 MB (while it is decoded) | — |

The WebAssembly module's own memory stays at 121–183 MB on the examples and on
the 12 MP photo, against ~230 MB before and the ~430 MB at which iOS Safari
closed the tab.

What keeps it low:
- the photo is held as bytes;
- stages work in place;
- masks are bytes;
- morphology never builds an image-sized temporary;
- the A4 page is built one channel at a time;
- the photo is dropped once the page is rectified.

The two WebAssembly builds give the same bytes: a PDF from `st` and from `mt`
has the same SHA-256.

## Build

```sh
cargo build --release                          # target/release/a4norm, one thread
cargo build --release --features par           # with threads (the Docker images)
cargo test --release
web/build.sh                                   # web/dist/{st,mt}
```

`web/build.sh` needs `wasm-bindgen-cli` at the version in `Cargo.lock`, and a
nightly toolchain with `rust-src`, because atomics need a std built with them.
It also:
- links the threaded build with a shared, imported memory (max 1 GiB);
- names the pool worker's module path, which a plain static server needs.

## Check

- `web/node-check.mjs dist/st` runs the one-thread module in Node on the
  examples.
- `web/check/` is a cross-origin-isolated page. Its worker is made from a blob
  URL, as malahov.io's is, and it runs both builds in Chromium or WebKit
  through Playwright:

```sh
node web/check/server.mjs 8765 &
node web/check/run.mjs chromium /examples/landing-invoice.webp /examples/specimen-card-front.jpg,/examples/specimen-card-back.jpg
```

- `A4MEM=1 a4norm ...` prints the peak memory of each stage.

## The browser API

```js
const m = await import(base + 'a4norm.js');
await m.default({ module_or_path: base + 'a4norm_bg.wasm' });
await m.initThreadPool(navigator.hardwareConcurrency);          // mt only
const r = m.process([{ bytes, name }], ['--format', 'jpg', '--dpi', '200'],
                    (stage, done) => {});                        // done 0..1, rising
// r.pages: [{ jpg, dpi }] (dpi also in each JPEG's JFIF header)
// r.report: the command line's stdout, "card: 1 ID-1 card ..." included
const pdf = m.pack(jpgs, new Uint32Array(dpis), gray);
```
