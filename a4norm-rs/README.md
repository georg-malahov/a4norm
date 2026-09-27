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

Some things depart from the script on purpose:
- **A card's colour copy takes its light from the card alone.** The script
  blurred the whole rectified rectangle, so the desk in the rounded corners
  darkened the estimate there and the division left a glow in the corners.
  The estimate is now a blur normalized by the card's rounded shape, set in
  by a hundredth of its width.
- **A blank page stays blank.** The tone stretch takes its black point from
  the darkest 0.1% of the page, meant to be ink. On a sheet with no ink (the
  back of an envelope) that is a crease at ~0.89, and the script stretched the
  paper ninefold into grey stains. The black point is now taken no lighter
  than half the paper; every inked page seen, a pencil notebook included,
  already sits at or below that, so they do not change.
- **A card must show its own edges.** A card-shaped region with one to three
  sides along the frame's border is a light background flooded together with
  what lies on it (a window envelope on a pale carpet), not a card. A picture
  cropped to a card, with all four sides there, still is one.
- **A fold is where two pages meet.** A sheet held in the hand, its edge
  covered by a finger or sagging, bends by 11-18° and leaves a gap of ~10% of
  the short side; a booklet's fold bends a few degrees and the halves meet.
  A kink over 15° or with a gap over 5% no longer splits a sheet into a spread.
- **Edges correct brightness.** A side of the brightness quad that lies on no
  edge moves in onto the strongest edge near it, if that edge is a step down
  from paper to something darker. This catches a hand or a pale carpet that
  was taken for paper. A printed rule has paper on both sides and does not
  qualify.
- **Receipts are found by their edges.** A receipt (2.5-6:1, 15%+ of the frame,
  75%+ of its outline on edges) is often greyer than the desk under it. It is
  taken by its outline whatever its colour. Of near-equal outlines, the
  outermost is used, so print at a crumpled edge is kept. A receipt gets no
  face photo.
- **A card's back is turned upright by its machine-readable zone.** The
  script turned a card by its face photo, which a back does not have, so a
  back shot upside down stayed upside down. The card is rectified at true
  size, so the MRZ can be read off its geometry: two or three lines of
  glyph-sized marks at one even pitch (OCR-B, 2.54 mm on a real card), over
  most of the width and a line apart. If they sit in the upper half, the card
  turns 180°. A back without an MRZ (a driving licence) stays as shot.
- **The document is found by its outline first.** Four straight lines, each a
  boundary over most of its length: the two sides of it differ in brightness,
  or in grain with the smoother side inside. Nothing asks what colour paper or
  desk is, so a pale passport on pale granite, where brightness saw one
  bright region, is found as it lies. The grain of granite, carpet or wood is
  taken out first with a median, which a straight edge outlives. All four
  sides count together, so the edge of a windowsill or a line of the page
  carried on over the desk makes no outline. Of outlines inside one another
  the larger wins unless it holds much worse, so a photo on a page does not
  beat the page. A fold is a cut across the middle third lying on an edge
  over 90% of its width; ruled paper has many such cuts and is no fold.
  When no outline holds (a sheet filling the frame, a crumpled page, a card
  whose sides run on) brightness decides, as before.
- **Nothing is turned by a guess.** The script turned a spread upright by the
  way its text ran and where its face photo sat; that turned passport pages
  wrong. The page stays as shot and `--rotate` (or the browser's `rotate`)
  turns it. A card is still laid landscape by its face photo or MRZ.
- **A spread's pages must be page-shaped** (1.1-2:1). A card whose MRZ band
  and tinted top split into two light regions is no longer taken for a
  passport spread.

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
- links the threaded build with a shared, imported memory (max 512 MiB);
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

// the Edit panel's rotate: quarter turns clockwise, dpi kept
const turned = m.rotate(jpg, 1, 88);
// the Edit panel's eraser: round spots filled from what surrounds them
const jpg2 = m.inpaint(jpg, new Float32Array([x, y, r, ...]), 88);   // page px
// the size control: pages written again, four at a time in parallel
const small = m.recompress(jpgs, { dpi: 150, quality: 60, gray: false });
// small: [{ jpg, dpi }]; dpi only ever goes down
// the camera's live outline: a video frame drawn to a ~600 px canvas
const { kind, quads } = m.detect(ctx.getImageData(0, 0, w, h).data, w, h);
// kind: "sheet" | "receipt" | "cards" | "spread" | "none"; quads: 8 numbers
// a quad, corners clockwise from the top left, in the canvas's pixels
```

`detect` runs the scan's own finder on one frame, so the outline drawn over
the camera is the one the scan will take. At 600 px it takes 110-220 ms on
one thread (Chromium, an M-series Mac), about five frames a second. Keep it
in a worker, send the next frame when the last answer is back, and ease the
corners between answers so the outline does not shimmer.
`web/check/camera.html` does all that, and scans a still with `process`:

```sh
node web/check/server.mjs 8765 &   # then open http://localhost:8765/camera.html
```

- **`inpaint`** fills the spots by push-pull. The known pixels are averaged
  down a pyramid until the holes close, then brought back up. The fill is a
  smooth membrane through the colours at the spots' rim, so it takes the
  page's own tone, white or a passport's grey or blue, shading included. Only
  a box round the spots is touched: a stroke of 11 dots of radius 70 takes
  5 ms. The page is written again at `quality` (default 88) with its dpi kept,
  and a grey page stays grey.
- **`recompress`** brings a page down to `dpi` (triangle filter) and writes it
  at `quality`. Colour goes 4:2:0 once the quality or the dpi drops. At
  today's settings the size is today's, so "normal" can stay the original
  bytes. The size presets, measured on an invoice, a notebook and a card
  page:

| preset | settings | size vs today | small print |
|---|---|---|---|
| normal | the page as made (200 dpi, quality 88) | 100% | — |
| smaller | `{ dpi: 200, quality: 70 }` | 57-62% | unchanged to the eye |
| minimum | `{ dpi: 150, quality: 60 }` | 32-37% | 6 pt still reads |

  Three pages take 0.12 s (smaller) and 0.18 s (minimum) in `st` on an M4.
