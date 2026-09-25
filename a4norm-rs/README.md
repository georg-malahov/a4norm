# a4norm-rs — stage 1 of the Rust port

In the browser a phone photo takes ~40 s on an iPhone 12 mini and ~200 s on a
budget Android (malahov.io#322). Most of that is ~30 separate ImageMagick
calls in WebAssembly, each reading and writing a full-size page. The plan is
one Rust source for the native binary (Docker, CLI, bot, skill) and the
browser's WebAssembly. This crate is the experiment that decides whether to
do it. It ports the heaviest full-resolution chain and measures it:

**flat_field → neutralize_ink → tone**, on a page that is already rectified.

The operators are ImageMagick's own, with its geometry:

- where `-sample` picks a pixel;
- the resize weights and axis order, Lanczos and Triangle;
- the Gaussian width and its three sub-samples per tap;
- the Octagon kernel's shape;
- Rec. 709 luma for `-colorspace gray`;
- `-contrast-stretch` on one histogram of the intensity;
- the mask composite's linear blend.

One more detail matters. The script's intermediate files keep the depth of the
source photo, 8 bits, so each stage hands the next one 8-bit values. The crate
rounds at the same points; without it the ink mask moved by 0.1% of the page.

## Result

Time for the three stages, best of 3–10 runs, on an Apple M4 Max. Every page
enters already rectified, so a 50 MP photo is a ~3.5 MP page here too. ImageMagick
native is the script's own calls, with process start-up and MIFF I/O. ImageMagick
WASM is the same commands in the browser's `magick.wasm` (7.1.2 Q16, no HDRI), in Node.

| page | IM native | IM WASM | Rust native, 1 thread | Rust native, 14 threads | Rust WASM | Rust WASM +simd128 | speed-up in WASM |
|---|---|---|---|---|---|---|---|
| Android 12 MP → 1534×2260 | 2.05 s | 3.04 s | 0.157 s | 0.053 s | 0.319 s | **0.215 s** | **14×** |
| Android 50 MP → 1534×2253 | 2.05 s | 2.91 s | 0.167 s | 0.053 s | 0.309 s | **0.228 s** | **13×** |
| examples/notebook-photo 917×940 | 0.65 s | 0.73 s | 0.041 s | 0.017 s | 0.089 s | **0.057 s** | **13×** |
| examples/landing-invoice 794×1124 | 0.63 s | 0.74 s | 0.041 s | 0.017 s | 0.081 s | **0.058 s** | **13×** |

Quality is PSNR at 8 bits against the script's own output of each stage:

| page | stage | Rust ↔ IM native (7.1.2 HDRI) | Rust ↔ IM WASM | IM WASM ↔ IM native, for scale |
|---|---|---|---|---|
| Android 12 MP | flat / neutral / tone | 96.1 / 95.8 / 84.5 dB | 76.8 / 75.2 / 67.9 dB | 76.8 / 75.2 / 68.0 dB |
| Android 50 MP | flat / neutral / tone | 96.3 / 95.1 / 81.7 dB | 76.9 / 76.6 / 71.5 dB | 76.9 / 76.6 / 71.1 dB |
| notebook-photo | flat / neutral / tone | 96.2 / 96.3 / 96.7 dB | 77.3 / 77.4 / 79.1 dB | 77.3 / 77.4 / 79.1 dB |
| landing-invoice | flat / neutral / tone | 95.7 / 94.4 / 107.6 dB | 77.4 / 77.3 / 81.1 dB | 77.4 / 77.3 / 81.1 dB |

- The coloured-ink share the script reports is the same to the last digit:
  15.399%, 15.439%, 0.046% and 0.852%.
- The Rust output is the same byte for byte natively and in WebAssembly, with
  or without SIMD and threads.
- Rust is as far from the browser's build as the browser's build is from native
  ImageMagick.
- Side by side the pages cannot be told apart. The difference amplified 33× is
  black.
- Debian's ImageMagick 7.1.1 in the `:full` image rounds its grey slightly
  differently: ~52 dB there, still invisible.

The module is 72 KB against magick.wasm's megabytes. It uses 128 MB of memory
for a 3.5 MP page against the 256 MB preallocated for ImageMagick.

**Verdict:** 13–14× on the step in the browser's runtime, above the 3× bar, at
the same output. Go on to stage 2.

Two caveats:

- These three stages were ~3 s of the ~12 s a 12 MP photo takes in the browser
  at 200 dpi. The rest is locating, rectifying, despeckling and the lay-out,
  which are the same kind of work and are what stage 2 moves.
- Threads in WebAssembly (`wasm-bindgen-rayon`) were not measured. They need a
  nightly toolchain and cross-origin isolation (COOP/COEP headers) on the site.
  Natively they give another 3×. For the browser that is a stage 2 decision, and
  single-threaded is already under target.

## Build and run

```sh
cargo test --release
cargo build --release --features cli            # target/release/a4norm-finish
cargo build --release --features cli,par        # the same, with threads
RUSTFLAGS="-C target-feature=+simd128" \
  cargo build --release --lib --target wasm32-unknown-unknown
```

The oracle is the script itself. `bench/dump.py` runs it and keeps the page
before and after each stage, plus the magick commands the stages ran:

```sh
python3 bench/dump.py ../a4norm /tmp/nb -- --format jpg -o /tmp/nb.jpg ../examples/notebook-photo.jpg
target/release/a4norm-finish /tmp/nb-in.png /tmp/nb-rs.png --compare /tmp/nb --min-psnr 60 --reps 5
MAGICK_WASM_DIR=.../malahov.io/public/a4norm-web node bench/imbench.mjs /tmp/nb 3
node bench/rsbench.mjs target/wasm32-unknown-unknown/release/a4norm_rs.wasm /tmp/nb 3
```

CI runs the comparison in both images, light and full, on the public examples.
`A4DBG=prefix` writes each intermediate mask as a 16-bit PGM, to find which
operator drifted.
