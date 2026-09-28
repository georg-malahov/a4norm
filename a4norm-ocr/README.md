# a4norm-ocr (spike, 2026-09-28)

Text lines on a page and their boxes, in pure Rust so the same code runs natively and in the
browser: PP-OCRv5 mobile detection + the latin recognizer, executed by
[tract](https://github.com/sonos/tract) 0.23. A spike for A4Norm Forms (fill a paper form from
an interview) and for a text layer in A4Norm's PDFs. Not wired into `a4norm-rs` yet.

## Models (not in the repo)

`models/` is git-ignored. Put three files there:

| File | Source | Size | License |
|---|---|---|---|
| `det.onnx` | [PaddlePaddle/PP-OCRv5_mobile_det_onnx](https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx) `inference.onnx` | 4.8 MB | Apache-2.0 |
| `rec.onnx` | [PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx](https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx) `inference.onnx` | 8.0 MB | Apache-2.0 |
| `dict.txt` | `PostProcess.character_dict` of the rec model's `inference.yml`, one entry per line (836; ä ö ü ß @ included) | 2 KB | Apache-2.0 |

The recognizer's classes are blank + the dictionary + a space (838).

## Run

```
cargo build --release
target/release/a4norm-ocr models ../examples/forms/demo-filled-scan.jpg [DET_LONG=960]
```

Browser module: `RUSTFLAGS="-C target-feature=+simd128" cargo build --release --lib --target
wasm32-unknown-unknown --target-dir target/wasm-st`, then `wasm-bindgen --target web`. API:
`new Ocr(det, rec, dict)`, `ocr.page(rgba, width, height)` → JSON
`{skew, lines: [{text, score, bbox}]}`.

## What it does

1. Detection on the page scaled to a long side of 960 px (multiple of 32), BGR, ImageNet
   mean/std; DB post-processing with the model's own settings (thresh 0.3, box_thresh 0.6,
   unclip 1.5) and axis-aligned boxes.
2. Skew: the median slope of the wide regions; above 0.1° the page is turned level
   (bilinear) and detected again. Boxes are in the levelled page.
3. Recognition of each box at height 48, padded to a width bucket (160 … 1920) so only a few
   shapes get compiled; greedy CTC decoding.

Loading needs `with_ignore_value_info(true)` and `with_ignore_output_shapes(true)`: the exported
graph carries symbolic shapes that tract cannot unify with a concrete input.

## Results on `examples/forms/demo-filled-scan.jpg` (200 dpi, turned 0.6°, noise)

| | native (M-series, 1 thread) | wasm32 + simd128 in Node, 1 thread |
|---|---|---|
| Whole page | 4.8 s | 8.9–10.0 s |
| Lines found | 40 of 40 | 40 |
| Skew found | 0.66° | 0.66° |
| Peak memory | — | ~340 MB RSS |

Text against the PDF's own text: ~6 wrong characters of 1842 (≈ 0.3 %), e.g. `behandeinde`
for `behandelnde`, a stray `"` and `-`, a missed `|`. Every field line reads right
(`Name: Greenholt`, `Straße, Hausnummer: 635 Fay Harbors`, `Becken/Hüfte li`, umlauts, `§`).
Without levelling, long lines of small print merged with their neighbours and came out as
garbage — levelling is required, or rotated boxes.

## Open ends

- The module is 16.8 MB before `wasm-opt`: tract registers every ONNX op. Try `opt-level = "s"`,
  `wasm-opt -Oz`, stripping, and whether tract can be built with fewer ops.
- Recognition dominates the time. Parallel lines (rayon in the threaded build), tighter
  width buckets, and mapping boxes through the skew instead of a second detection.
- Boxes are axis-aligned; rotated boxes (min-area rectangles) would remove the levelling pass.
- Only latin. Cyrillic/Arabic recognizers exist in the same family
  (`cyrillic_PP-OCRv5_mobile_rec_onnx`, `arabic_…`) if ever needed.
