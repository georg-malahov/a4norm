# a4norm-fill — a form filled in, no models

Where each answer goes and how large (`fill`), and the filled form as a PDF over its own
pages or a scan's pictures (`pdf`), in Arimo (`font`, `fonts/`, SIL OFL 1.1). No tract, no
models. The OCR module (`a4norm-ocr`) uses this crate for its own `fillLayout`, `fillPdf`
and `fillScan`, so there is one implementation; how it places and writes is described in
a4norm-ocr's README ("Where the answers go", "The filled form as a PDF").

## The free form mode (D40)

The free mode fills a form in by hand, on the device. It needs two modules:
- the scanner's `a4norm.wasm`, already loaded to scan, for `formGeometry`;
- this module, `a4norm_fill.wasm`, for the layout and the PDF.

The OCR module and its models are not needed.

```js
import initFill, { fillLayout, fillPdf, fillScan } from "./fill/a4norm_fill.js";
await initFill();
// a page the scanner made: its fields, each with `id`, `empty`, a field's `label`
const g = formGeometry(r.pages[0]);          // a4norm.wasm: { jpg, dpi } -> { sizePt, lines, rects, combs, boxes, … }
const fields = [{ key: "p1-12", type: "text", place: { page: 1, candidate: 12 } }, …];
const request = JSON.stringify({ inspections: [{ ...g, page: 1 }], template: { fields }, answers,
  images: [{ page: 1, box, rotate: 90 }], texts: [{ page: 1, x, y, size: 11, text, rotate: 15 }] });
const layout = fillLayout(request);                  // {baseSize, placed}
const { pdf } = fillScan(request, [r.pages[0].jpg], [signaturePng]);   // or fillPdf(request, pdfBytes, [], …)
```

- `formGeometry`'s candidates are the OCR module's, with the same ids, found without words.
- Each candidate also carries `empty`, and a field its printed label's foot, `label`. A value
  set in a field goes below that label, as it goes below the words the OCR reads.
- **Rotation.** `images[].rotate` and `texts[].rotate` turn a picture or a text clockwise,
  in degrees, about its middle:
  - a picture about its box's middle;
  - a text about its middle as set: its width in Arimo, from the capitals' top to the
    descenders' foot.

  Turned, the picture is still the one image and the text is still text. Without `rotate`
  the PDF is as before, byte for byte.

## Build

```sh
a4norm-fill/web/build.sh [OUT]      # default a4norm-rs/web/dist/fill: a4norm_fill.js, a4norm_fill_bg.wasm
node a4norm-fill/web/node-check.mjs SCAN_DIST/st OCR_DIST/st FILL_DIST
```

The build is one module, on one thread (filling needs no threads). It is made at
opt-level "s", then `wasm-opt -Oz`.

`node-check.mjs` runs the free mode on the demo and on KG 1 p. 2, with a signature at 90°
and a note at 15°. It checks that the scanner's `formGeometry` (without `empty` and
`label`) and this module's `fillLayout`, `fillPdf` and `fillScan` are byte for byte the OCR
module's.

## Size

| | wasm | gzip |
|---|---|---|
| `a4norm_fill_bg.wasm` | 1 742 KB | 817 KB |
| of which Arimo (`include_bytes`) | 478 KB | 263 KB |
| `a4norm_fill.js` | 12 KB | 3 KB |

The rest is:
- lopdf, to read and write PDFs, with its AES, brotli and encoding tables: forms such as
  KG 1 and Bavaria's Wohngeld open only decrypted;
- PNG and JPEG decoding, for the pictures set over the page;
- the font subsetter, ttf-parser, serde and std.

`subsetter` is built without variable fonts: Arimo is static, and skrifa and write-fonts
took a third of the module. The OCR module lost the same 0.6 MB.

For comparison:
- `formGeometry` added 8.5 KB (3 KB gzip) to `a4norm.wasm`, whose geometry was already
  there for `looksLikeForm`;
- filling inside `a4norm.wasm` would have added this module's 1.7 MB to every scan.
