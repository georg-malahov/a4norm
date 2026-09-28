# a4norm-ocr

A page of a form as A4Norm Forms (filling in a paper form from an interview) needs it: its
words and where they sit, and where it is to be filled in. Pure Rust, so the same code runs
natively and in the browser. The words come from PP-OCRv5 mobile detection and the latin
recognizer, run by [tract](https://github.com/sonos/tract) 0.23. The geometry is plain image
work and needs no model.

It is a module of its own, apart from `a4norm.wasm`: scanning never downloads it, and the
models are fetched separately. It gives A4Norm Forms' `PageInspection`, in points from the
top left:

| Field | What it is |
|---|---|
| `sizePt` | the page's size in points, as the caller gives it |
| `skewDeg` | the tilt of the text lines, degrees; positive runs down to the right |
| `words` | `{text, bbox, score}` in reading order; `bbox` upright around the word |
| `printedSize` | the size of the page's print in points: the median over characters of each line's height |
| `langs` | the page's languages, most frequent first; a label in four languages gives all four |
| `lines` | writing lines, dotted or solid: `{id, x0, y0, x1, y1}` through the middle of the stroke |
| `rects` | fields and table cells: `{id, box}`, the white inside within the rules |
| `combs` | rows of cells for a character each (tax ID, IBAN, BIC, dates): `{id, box, cells}` |
| `boxes` | check boxes, square or round: `{id, box}`, the inside |
| `typicalFieldHeight` | the median of the fields' heights: the room above a line, a field's or a comb's inside, a box's side |

Every candidate (line, field, comb, box) has an `id`, 1, 2, … in reading order over all
four kinds, so that a model can name them ("candidate 12 is the date of birth"). The page
does not supply `page`; the caller does.

## Models (not in the repository)

```sh
a4norm-ocr/models.sh        # into a4norm-ocr/models/, checked by sha256
```

| File | Source (pinned revision) | Size | Licence |
|---|---|---|---|
| `det.onnx` | [PaddlePaddle/PP-OCRv5_mobile_det_onnx](https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx) `inference.onnx` | 4.8 MB | Apache-2.0 |
| `rec.onnx` | [PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx](https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx) `inference.onnx` | 8.0 MB | Apache-2.0 |
| `rec.yml` | the same, `inference.yml`: its `character_dict` is the recognizer's 836 characters (ä ö ü ß § € @ among them) | 7 KB | Apache-2.0 |

The recognizer's classes are the CTC blank, the dictionary, then a space (838). The graphs
carry symbolic shapes that tract cannot unify with a concrete input, so loading drops them
(`with_ignore_value_info`, `with_ignore_output_shapes`).

## Use

```sh
cargo build --release
target/release/a4norm-ocr models ../examples/forms/demo-filled-scan.jpg      # PageInspection JSON
target/release/a4norm-ocr models ../examples/forms/demo-blank-a4norm-scan.pdf --lines
target/release/a4norm-ocr models page.png --mark marked.png                  # the candidates drawn, numbered
target/release/a4norm-ocr --geometry page.png [marked.png]                   # the geometry alone, no models
```

In the browser (`web/build.sh` builds `st/` and `mt/` into `a4norm-rs/web/dist/ocr`):

```js
import init, { Ocr, initThreadPool, releaseThreadPool } from "./ocr/mt/a4norm_ocr.js";
await init();
await initThreadPool(navigator.hardwareConcurrency);   // mt only, as in a4norm
const ocr = new Ocr(detBytes, recBytes, recYmlText);   // once
const page = ocr.inspect(imageData.data, width, height, 595.28, 841.89);  // a canvas's RGBA
const scan = ocr.inspectImage(jpegOrPngOrScanPdfBytes, 595.28, 841.89);
await releaseThreadPool();                             // soon after the work, see a4norm-rs/docs/threads.md
const geometry = formGeometry(imageData.data, width, height, 595.28, 841.89);  // no models
```

`inspect` gives the geometry the words it read: a cell inside a word is a letter, a line
along a word is the tops of its letters, and a frame holding three lines of words is a
note, not a field. `formGeometry` works without them, and a large letter or a note in a
frame can then pass for a field.

The threaded build's pool is the one a4norm-rs has (`src/pool.rs`, `src/pool.js`, kept
alike), with the same rules: release it soon after the work. Without a pool every call runs
on one thread and gives the same result.

## How it reads a page

1. **Detection** on the page scaled to a long side of 960 px (multiples of 32), BGR,
   ImageNet mean/std. DB post-processing with the model's settings (thresh 0.3, box_thresh
   0.6, unclip 1.5).
2. **Skew and tilted boxes, one detection.** The skew is the median slope of the long
   regions. Every box is its region's extent in the levelled frame, so a tilted line of
   small print gets a thin tilted box instead of an upright one that takes in its
   neighbours. Where lines are set tight the grown boxes still overlap, so two boxes
   side by side across the lines part halfway between their regions. Without this, a
   line cut out with a strip of its neighbour reads as garbage.
3. **Recognition.** Each line is cut out along its box (bilinear) to a height of 48. It
   is padded to the first of 15 widths about 1.25 apart (64 … 1920): each width is a
   compiled plan, and a line pays for its padding. A symbolic width would need one plan,
   but runs 3–4 times slower in tract. With `par` (the `mt` build) the lines are cut and
   read in parallel.
4. **Words.** Greedy CTC decoding keeps the frame of each character: a word runs from
   half a character before its first one to its last frame, and splits at the
   recognizer's spaces.
5. **Print size**: each confidently read line's box height over 1.39, the median over
   characters, so body text outweighs a few headings. **Languages**: the function words
   and label words of de, en, fr, it, es, pt, nl, pl, hr and tr. A word on more than one
   list is not counted. A language is kept if it has at least 3 words and a quarter of the
   top language's count.

## Where a form is filled in (`src/geometry.rs`)

All of it is built from strokes, measured in points, so the resolution does not matter;
the rules were set at 200 dpi.

1. **Ink**: darker than 70 % of the paper, which is taken as the 95th percentile of
   brightness.
2. **Strokes.**
   - Horizontal strokes are runs of ink along a row at least 4 pt long, with the dots of a
     dotted line joined across gaps of up to 2.2 pt.
   - Vertical strokes are unbroken runs down a column, at least 3 pt long.
   - Whatever is thicker than 2.5 pt across is dropped.
   - The rest is fitted piece by piece with a straight line, so a tilted page is no
     trouble.
   - A horizontal stroke with ink running right along it, above or below (the letters of a
     line of text, a heading on its rule), is text. A parallel rule does not count: a
     doubled line is not text.
3. **What stands on a horizontal stroke.** Vertical strokes standing on it (thin rules at
   least 9 pt high) at both of its ends make it the bottom of a row of cells. The top may
   be open, as on the Familienkasse's forms.
   - Narrow, even cells side by side make a **comb**; a doubled bottom rule finds the row
     once.
   - The other cells are **fields**. A field is dropped if it:
     - holds other candidates (a frame around a group);
     - holds three lines of words, or words over two thirds of it (a note);
     - is small with something in its middle (a section's number);
     - lies inside a word (the inside of a large "U").
4. **Writing lines** are the other horizontal strokes, at least 32 pt long. A stroke with
   rules hanging from both ends is the top of a box, not a writing line. A line broken in
   print (by dots lost) is joined across up to 8 pt. A field's doubled bottom rule is not
   a line.
5. **Check boxes** are small enclosed white areas (2.8–26 pt, square or round), empty in
   the middle. Looking out four ways from the middle, the border must be thin, at most a
   quarter of the side and at most 1.6 pt, with paper right beyond it on three sides. A
   letter's counter, as in a bold "o" or an "O" in a heading, fails this.
6. **Numbers.** Rows are formed by the bottom edge, a new row where it drops by 6 pt from
   the row's first; within a row, left to right.

**The demo form, three ways.** The same 20 writing lines and 4 boxes, numbered the same, on
each of:
- the vector PDF rendered at 200 dpi;
- the synthetic scan, turned 0.6° with noise;
- the phone scan of the printout.

The 20 lines are the 14 dotted ones and 6 solid ones: the date, the signature, both sides
of "Anmerkungen", both sides of the last rule. That is the AcroForm's 20 text fields.

The positions differ as the pictures do:
- The synthetic scan is turned, and the printout was shrunk 1.4 % in print. So a line at
  the bottom of the page lies 8–9 pt from where it is on the PDF.
- Once one picture is laid on the other (the best turn, scale and shift over all
  candidates), every end is within 3.6 pt, and across the lines within 1.1 pt. The median
  is 0.3–0.9 pt.
- `typicalFieldHeight` is 31.4–32.2 pt on the three, and `printedSize` 10.9–11.2 pt.

**Official forms** (first pages with fields, 200 dpi; `docs/geometry-*.jpg`, drawn by
`--mark`):

| Form | Lines | Fields | Combs (cells) | Boxes |
|---|---|---|---|---|
| Familienkasse KG 1, p. 2 | 4 | 20: every field, open on top | tax ID 2+3+3+3 twice, IBAN 34, BIC 11 | 9 |
| Jobcenter HA, p. 1 | 6: the staff's 5, a heading's rule | 14: every field, rounded | — | 6: 4 square, Ja/Nein round |
| Frankfurt Aufenthaltstitel, p. 1 | 17: the 14 dotted, the rule after "getrennt lebend seit", the table's top and bottom | — | — | 9: the character "□" |

A field drawn as a bracket (a bottom rule with ends only 5 pt high, as for KG 1's
"Kindergeld-Nr.") is found as its writing line. Without the words, Jobcenter's note in a
frame passes for a field, and Frankfurt keeps two strokes along a heading.

## Results of the reading

On the demo form (`examples/forms/`, `cargo test --release`). The truth is the text of the
PDF the page came from (`demo-*.txt`); the error is order-free: characters one text has
more of than the other, the larger side.

| Page | Lines lost | Characters wrong | Skew | Print | Langs |
|---|---|---|---|---|---|
| `demo-filled-scan.jpg` (synthetic, 0.6°, noise) | 0 of 37 | 6 of 1842 (0.33 %) | 0.66° | 11.2 pt | de |
| `demo-blank-a4norm-scan.pdf` (printed, phone scan) | 0 of 36 | 4 of 1699 (0.24 %) | −0.03° | 11.2 pt | de |

The misreads are a stray `"` and `-` around a line, `behandeinde` for `behandelnde`, and
`Patientin` for `Patient/in` at 8 pt. Every value on the filled form reads right.

Languages on the first page of official forms (`examples/forms/official.json`, rendered at
200 dpi):

| Form | `langs` |
|---|---|
| Berlin, Aufenthaltstitel (DE/EN/FR/IT) | en, it, fr, de |
| Frankfurt, Aufenthaltstitel (DE/EN/FR/ES/HR/TR) | es, en, fr, tr, de, hr |
| München, Aufenthaltstitel (DE/EN) | de, en |
| Familienkasse, KG 1 | de |

**Time per page**, 200 dpi A4, the demo scans. The first page also compiles the plans; the
times shown are the later pages. Apple M4 Max, 14 cores, the machine shared with other
work, so they vary by ~10 %:

| | demo-filled-scan | demo-blank-a4norm-scan |
|---|---|---|
| native, one thread | 4.0 s | 4.0 s |
| native, `par` (14 threads) | 0.81–0.83 s | 0.73–0.74 s |
| wasm `st` in Node 20 | 7.4–7.9 s | 7.1 s |
| wasm `st` in Chromium / WebKit (a worker) | 7.4 / 7.6 s | 7.1 / 7.1 s |
| wasm `mt` in Chromium / WebKit (a worker, 14 threads) | 1.5 / 1.5 s | 1.2 / 1.4 s |
| first page, `mt` (plans compiled) | 7.0 / 4.4 s | |

`mt` needs Web Workers, which Node lacks, so it is measured in the browsers
(`web/check.mjs`). In a browser the models load in ~20 ms. `initThreadPool` takes
10–33 ms and `releaseThreadPool` 1–2 ms. Node holds ~210 MB RSS for a page in `st`.

**Size.** The spike's module was 16.8 MB. It is now built at opt-level "s", with tract's
matrix kernels (`tract-linalg`) at 3, then `wasm-opt -Oz`:

| | wasm | gzip -9 |
|---|---|---|
| `st/a4norm_ocr_bg.wasm` | 8.2 MB | 2.4 MB |
| `mt/a4norm_ocr_bg.wasm` | 8.2 MB | 2.4 MB |
| models (fetched apart) | 12.9 MB | |

At opt-level 3 throughout the module is 13.3 MB and slower in the browser (8.1 s against
7.4 s). At "s" throughout it is 7.9 MB but takes 12–13 s.

```sh
node a4norm-ocr/web/node-check.mjs [DIST/st]                    # Node, one thread
node a4norm-ocr/web/check.mjs chromium mt [THREADS] [DIST]      # a browser, as the site runs it
```

## Open ends

- Only latin. Cyrillic and Arabic recognizers exist in the same family
  (`cyrillic_PP-OCRv5_mobile_rec_onnx`, `arabic_…`).
- A page tilted differently in its parts (a photo bent at the spine) gets one skew for all
  its lines.
- The first page compiles a plan for each width it needs (~60 ms natively each).
