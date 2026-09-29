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
const layout = fillLayout(JSON.stringify({ inspections, template, answers }));  // where the answers go
const filled = fillPdf(JSON.stringify({ inspections, template, answers }), pdfBytes, []);
```

`inspect` gives the geometry the words it read, with where each character was read: a
box inside a word is a letter's loop when a letter or digit was read in it ("6" in
"635"), a line along a word is the tops of its letters, and a frame holding three lines of
words is a note, not a field. A row of options read as one word over its boxes ("□ männlich
□ weiblich" as "männlichweiblich", Jobcenter Hauptantrag p. 1) has nothing read in the
boxes, so they stay boxes; a round one read as a letter with its label ("◯ ja" as "Oja")
stays a box when a word space (1.5 pt of paper) follows it, where a letter's next is
closer ("Ocupación").

In turn, the geometry parts the words. A row of check boxes is read as one word
("zu:JaNein" over "zu: [] Ja [] Nein"), so the word is split at each box, by where each
character was read, and each option's label becomes a word of its own. `formGeometry` works without them, and a large letter or a note in a
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

## Where a form is filled in (`a4norm-geometry`)

The geometry is a crate of its own, `a4norm-geometry/` (it needs only `image`), used here
as `a4norm_ocr::geometry` and by the scanner's browser module for `looksLikeForm`.

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
4. **Writing lines** are the other horizontal strokes, at least 32 pt long.
   - A stroke with rules hanging from both ends is the top of a box, not a writing line.
   - A line broken in print or cleaning, by dots lost, is joined across up to 8 pt, or up
     to 16 pt with paper above the gap. Two lines with a label between stay two.
   - A field's doubled bottom rule is not a line.
5. **Check boxes** are small enclosed white areas (2.8–26 pt, square or round), empty in
   the middle.
   - Looking out four ways from the middle, the border must be thin, with paper right
     beyond it on three sides. Thin means at most a quarter of the side, 0.6–2.4 pt: the
     scanner's cleaning leaves rules bolder. A letter's counter, as in a bold "o" or an
     "O" in a heading, fails this.
   - A round one under 7.5 pt is a loop, as of a "6" or an "o"; a box that small is
     square ("□").
   - With the words read, a box inside a word and lower than it is a letter.
6. **Numbers.** Rows are formed by the bottom edge, a new row where it drops by 6 pt from
   the row's first; within a row, left to right.

**After the scanner.** A photo of paper reaches A4Norm Forms as the scanner leaves it:
cropped, levelled, cleaned, a 200 dpi JPEG. Both demo scans come through it (looks
auto, magic, color) with their 4 boxes and no other (`tests/geometry.rs`). The cleaning
can take a faint rule with it: on the phone scan it removes the last rule of the page.

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

### Does a page look like a blank form? (`looks_like_form`, D24)

After "Process", the scanner offers to fill a page in when it looks like a blank form.
`geometry::looks_like_form(page, px_pt)` finds the page's candidates (no words, no
models) and counts the empty ones. In the browser this is the scanner's
`looksLikeForm(page)` (a4norm-rs/docs/api.md), so a plain scan never loads this module.

- Counted are the fields, combs, check boxes and the writing lines shorter than 400 pt.
  Longer lines are a table's rules or a letter's lines, and are not counted.
- An element is empty when under 4 % of it is dark (luma 0.3 R + 0.59 G + 0.11 B under
  128), measured in:
  - a field's inside, 1.5 pt in from its strokes, only the lower 55 % of it when it is
    higher than 14 pt (the printed label sits at the top: KG 1's "Familienname");
  - a comb's inside, 1.5 pt in, and a check box's, 1 pt in;
  - a line's band above it, 0.7 of the typical field high (8 to 18 pt), without the line.
- The result is `{empty, total, lines, rects, combs, boxes}`. The site offers the form
  when `empty >= 6` and `empty >= 0.6 × total`.

On the plan's set (section 15: the demo in 3 forms and 14 official forms, 2 pages each,
31 pages; the filled demo twice; 17 pages that are no forms: notes pages, the examples'
photos after the scanner, a synthetic letter, contract and invoice), `empty` and `total`
match the site's measurement within 1 on all 50. 29 of the 31 blank pages are offered;
the 2 misses are Finanzamt ESt 1 A (small dense cells). None of the 17 others is
offered, and neither is the filled demo (10 of 21 empty). In the browser's one-thread
module (Node, 200 dpi, the JPEG decoded too) a page takes 70 ms median, 87 ms at most,
once warm; the first call takes up to 170 ms.

## Where the answers go (`src/fill.rs`)

`fillLayout(requestJson)` (and `fill::layout` in Rust) places a form's answers on its pages
without drawing them; no models are needed.

The request is `{inspections, template, answers, color?, minSize?, images?, texts?}`:
- `inspections` are the pages' `PageInspection`s as `inspect` gave them;
- `template.fields` are `{key, type, place, options?}`. A `place` is `{page, candidate}`
  (a candidate's `id` on that page, as the model's structure names it), or `{page, box2d}`
  (`[ymin, xmin, ymax, xmax]` in 0–1000 of the page, for a field the geometry did not
  find), or `{page, box}` in points. Pages count from 1;
- `answers` maps a key to a text, a choice's option `value`, or `true` for a single box;
- `texts: [{page, x, y, size, text}]` are free text of the person's own, set level in the
  embedded font from its baseline's start `(x, y)`, in points from the top left.

**By hand** (the site's adjusting of a filled form). A field may carry:
- `size` (pt): its value is set at that size exactly, not made smaller, and left out of the
  page's shared size. It still breaks over two lines when one is too wide.
- `shift: [dx, dy]` (pt, right and down): all the field puts on its page moves by that
  much, after it has been placed: a text, a comb's characters, a choice's cross, and
  `placed`'s `x`, `y`.

Without them everything is as below.

The result is `{baseSize, placed: [{key, page, x, y, size, lines, overflow, kind,
candidate?, lost?}]}`:
- `x`, `y` are the first line's baseline at its start, or a cross's middle, in points from
  the top left;
- `kind` is `text`, `comb` or `check`;
- `lost` says that a character the font lacks (not in Arimo: Chinese, say) became "?".

**Snapping a drawn box.**
- A choice snaps to the check box it overlaps.
- A text snaps to a comb or field that holds half of it.
- Else it snaps to the writing line under it: overlapping by 14 pt, its foot within 25 pt.
- Else the box itself is the field.

A choice's options without places of their own are the check boxes of the field's row,
left to right.

**Sizes (the plan's amendment 3).**
- One size for the document: `base = clamp(round(printed + 2), 9, 0.72 × field)`, from the
  medians of the pages' `printedSize` and `typicalFieldHeight`. It is below 9 pt only when
  the fields are that low.
- A value that does not fit is made smaller, not below 7 pt (`minSize`), to a step of a
  short ladder: `base`, `base − 1`, … Fields whose room differs by a few tenths of a point
  take the same size.
- When more than half the text values of a page had to be made smaller, the others come
  down to the step most of those took, unless their field is notably higher (half again
  the usual room). On KG 1, p. 2 that leaves two sizes: 11 pt in the fields, 12 pt on the
  one open line. Combs and boxes keep their own sizes.
- Then it is broken over two lines at the space that evens them, if the field is
  2 × 1.15 × size high.
- Else it is set at 7 pt with `overflow: true`.

**Where it sits.**
- On a writing line the descenders clear the line by 0.8 pt, and the text follows the
  line's tilt.
- In a field the text is centred in what its printed label leaves.
- In a comb each character is centred in its cell, whitespace dropped; more characters
  than cells overflow.
- A check box gets a cross, 18 % in from its sides.
- The colour is `#1a1a1a` unless `color` says otherwise.

A field can take several candidates in order (`{page, candidates: [4, 5, 6, 7]}`):
- a value runs through the cells of several combs one after another (a tax ID printed in
  groups, 2 + 3 + 3 + 3);
- or it runs over lines and fields: as many words into each as fit at the base size, the
  rest into the next.

In a field, the text goes below every printed word inside its frame, such as the label
"Familienname" at the top of KG 1's fields. One line needs only its glyphs' height (cap to
descender, 0.925 em, and a little air); two lines need 2 × 1.15 × the size.

On the demo form (`examples/forms/demo-template.json`, `demo-answers.json`) every value of
`demo-truth.json` is set at 13 pt, within 2.9 pt of where the form's own app printed it on
the PDF, within 3.3 pt on the synthetic scan and within 4.4 pt on the phone scan. The two
scans are compared once laid on the PDF.

## The filled form as a PDF (`src/pdf.rs`)

```js
const r = fillPdf(JSON.stringify({ inspections, template, answers, images }), pdfBytes, pagePictures, imageBytes);
const r = fillScan(JSON.stringify({ inspections, template, answers, images }), pageJpegs, imageBytes);
// r: {pdf: Uint8Array, fallback, baseSize, placed}
```

**From a scan.** Each page is its JPEG over the page's `sizePt`, with the answers over it
as vector text.

**From a PDF.** The result is a new PDF with the source's pages, untouched, as Form
XObjects, and the answers over them as vector text:
- The coordinates are those of the page as inspected: its crop box, turned by its
  `/Rotate`.
- An encrypted source that opens without a password (AES, an empty user password and
  owner bans, as with the Familienkasse's forms and Bavaria's Wohngeld) is decrypted, and
  the copy is written without encryption. The original file is not changed.
- The form's own fields are not kept: no AcroForm, XFA or widgets, so nothing of theirs
  (a blue font, a value) shows over the answers. What they draw on the page is kept as
  drawn: KG 1's comb cells and box borders are its fields' appearances, stamped into the
  page.
- When the source cannot be read at all, `pagePictures` (JPEG, the pages as inspected)
  stand in, as for a scan, with `fallback: true`. Without them it is an error.

**Pictures over the page** (a signature from a pad, a stamp):
- The request's `images: [{page, box, key?}]` place them. `box` is in points from the
  page's top left; each picture is fitted in its box, its shape kept, in the middle.
- Their bytes (PNG or JPEG) come apart, in the same order: `fillPdf`'s and `fillScan`'s
  last argument, optional.
- A PNG's alpha becomes a soft mask, so a signature has no white box around it.
- The page stays vector: each picture is an image XObject of its own. The file grows by
  about the picture's size: 5.3 KB of PNG signature → 5.9 KB.

**Text.** Arimo (`fonts/`, SIL OFL 1.1; Arial's metrics), with Latin Extended, Cyrillic and
Greek, so "Yılmaz", "Şahin", "Łukasz", "Đorđević", "Ștefan" and "Müller-Straße" are set as
written. The module carries the font, and a PDF embeds only the glyphs its answers use: a
CID font with a ToUnicode map, so the text copies as written. For the demo form that is
~10 KB. The colour is `#1a1a1a`; a check box gets a cross.

On the demo form, filled with its 12 values and two choices:

| Source | Bytes in → out |
|---|---|
| `demo-blank.pdf` | 4 800 → 14 951 |
| `demo-blank-encrypted.pdf` | 5 220 → 14 848 |
| `demo-blank-acroform.pdf` | 11 793 → 15 644 |
| `demo-blank-a4norm-scan.pdf` (the JPEG) | 530 985 → 541 673 |
| KG 1, 5 pages, AES, static XFA | 1 508 259 → 932 118 (XFA and fields dropped) |

Of the ~10 KB added, ~9 KB are the embedded glyphs.

- The form's text stays selectable.
- The three demo PDFs give the same page, pixel for pixel in poppler and in pdf.js.
- Nothing blue.
- pdf.js in Chromium and WebKit and PDFKit (Preview and Safari's engine) show the same
  page (`docs/fill-*.jpg`, the three side by side).
- On KG 1, p. 2, every dark pixel of the form as shown is still dark once filled.

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
| `st/a4norm_ocr_bg.wasm` | 10.2 MB | 3.3 MB |
| `mt/a4norm_ocr_bg.wasm` | 10.2 MB | 3.3 MB |
| models (fetched apart) | 12.9 MB | |

The OCR alone came to 8.2 MB (2.4 MB gzip). The geometry adds ~60 KB; filling and writing
the PDF (serde, lopdf) ~720 KB; the font and its subsetting (Arimo, ttf-parser, subsetter)
~1.2 MB. At opt-level 3 throughout, the OCR module was
13.3 MB and slower in the browser (8.1 s against 7.4 s); at "s" throughout it was 7.9 MB
but took 12–13 s.

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
