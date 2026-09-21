# a4norm

A photo of a paper document is not a scan. `a4norm` makes it one: the sheet is
found in the frame and warped flat, the desk and the spiral binding are erased,
the camera's uneven light becomes even white paper, the ink goes neutral while a
blue signature or a red stamp stays coloured, and the page comes out as an exact
A4 PDF of a few hundred kilobytes. Several photos combine into one multi-page
document, because a paper document is rarely one page.

A frame that holds no document is recognised as such and left alone: the
picture is fitted onto the page as shot, without any of the scanner treatment.
So the same command also just turns a handful of snapshots into a PDF.

No OpenCV, no NumPy, no ML, no ghostscript. The tool is stdlib-only Python
driving ImageMagick and poppler, and the container is 104 MB.

```bash
docker run --rm -v "$PWD:/work" ghcr.io/georg-malahov/a4norm:latest \
  -o /work/contract.pdf /work/page1.HEIC /work/page2.HEIC /work/page3.HEIC
```

A synthetic test page lives in `examples/sample-photo.jpg` — a generated letter,
degraded to look photographed (warm cast, uneven light, a tilt, a desk border),
with no real data in it. A one-line smoke test:

```bash
docker run --rm -v "$PWD:/work" ghcr.io/georg-malahov/a4norm:latest \
  --preview /work/examples/sample-photo.jpg
```

## Run it

**Container** (nothing to install):

```bash
# one photo -> photo-A4.pdf next to it
docker run --rm -v "$PWD:/work" ghcr.io/georg-malahov/a4norm:latest /work/photo.HEIC

# several photos -> one multi-page PDF
docker run --rm -v "$PWD:/work" ghcr.io/georg-malahov/a4norm:latest \
  -o /work/doc.pdf /work/p1.jpg /work/p2.jpg

# analyze and report, write nothing
docker run --rm -v "$PWD:/work" ghcr.io/georg-malahov/a4norm:latest \
  --dry-run /work/photo.HEIC
```

**Locally**, if you already have the dependencies:

```bash
brew install imagemagick poppler                                             # macOS
apk add python3 imagemagick imagemagick-heic imagemagick-jpeg poppler-utils   # Alpine
apt install python3 imagemagick poppler-utils                                 # Debian 13+

./a4norm --preview photo.HEIC
```

Debian 12 and Ubuntu 24.04 still ship ImageMagick 6, which has no `magick`
binary — use the container there.

Input: JPG, PNG, HEIC, TIFF, WebP, and PDFs that are really just a photo
(including multi-page). Output: an A4 PDF at 300 dpi by default, or JPEG with
`--format jpg`.

## HTTP service

```bash
docker run --rm -p 8080:8080 ghcr.io/georg-malahov/a4norm:latest serve
```

| | |
|---|---|
| `GET /health` | `{"status":"ok", …}` |
| `POST /scan` | one or more images → `application/pdf` |
| `POST /scan?format=jpg` | a single image → `image/jpeg` |

Send images as `multipart/form-data` (any field names, repeat for several pages,
page order = part order) or, for one image, as a raw body with an image content
type. Any `a4norm` flag can be passed as a query parameter (`?dpi=200&gray=1`,
underscores allowed: `?paper_thr=85`). The response carries `X-Pages` and
`X-Seconds`.

```bash
curl -X POST http://localhost:8080/scan \
  -F p1=@page1.HEIC -F p2=@page2.HEIC -o document.pdf
```

Concurrency is capped at one page at a time by default (`--max-concurrency`):
a page is tens of seconds of CPU, so an unbounded server is a denial-of-service
switch. Bodies over 40 MB are refused, a queued request waits 120 s for a slot
before `503`, and a job is killed after 600 s.

## What it does, in order

1. **Rasterize** — a PDF's embedded image is extracted rather than re-rendered;
   rendering applies the ICC profile and flattens the tonal range.
2. **Rectify** — the sheet is the big bright low-chroma region, both tests
   relative to the image's own paper level, so a dim photo works like a bright
   one. Its quadrilateral is warped flat. A quad is accepted only if it looks
   like a sheet (15–90% of the frame, filling ≥80% of its hull, corners 45–135°,
   opposite sides within 1.8×); otherwise the reason is printed and the rest of
   the pipeline carries on, because rectifying on a wrong quad is worse than not
   rectifying.
3. **Or decide there is no document at all.** If no sheet quad was accepted
   *and* the paper-like area is under `--photo-paper` (20%), the frame is a
   photo, not a page: everything below is skipped and the picture is fitted
   onto the page as shot, at `--photo-dpi` (200) and `--photo-quality` (82)
   with 4:2:0 chroma — text needs full chroma and 300 dpi, a snapshot does
   not. A landscape photo turns the page instead of being rotated. Measured
   paper-like area: 9% for a photo of a desk, 40–97% for every real document,
   so the threshold sits in a wide gap. `--photo off` forces the scanner
   treatment, `--photo on` forces the short path.
4. **Erase what leans in from outside the sheet** — non-paper *connected to the
   frame edge* is desk, shadow or binding; sheet content cannot reach the border.
   It is flooded from the border and repainted in the page's own paper tone, and
   a thick band of it (a binding) is cropped away.
5. **Flat-field** — divide by a smoothed background estimate, in colour, which
   both evens the light and white-balances the paper.
6. **Deskew** above 0.4°, under 5°, after the flat-field (before it, a dim photo
   binarizes into one blob and a real tilt measures as 0.0°). Skipped after a
   rectify, which already set the orientation.
7. **Neutralize the ink** — a photo tints black print warm. Everything goes
   neutral except pixels that are both high-chroma and dark: real coloured ink,
   any hue.
8. **Tone** by histogram percentiles, then erase bright featureless haze (a soft
   shadow or a finger goes; anything with structure survives), then clean the
   paper to pure white with a 1 px guard ring around every glyph.
9. **Fit to A4** — from the real sheet edges when two opposite ones are visible
   (exact px-per-mm, no assumption about the layout), otherwise from the ink
   block and standard margins, otherwise the frame.

`--dry-run` prints which path each page took and why. Every parameter above is a
flag; `--help` lists them.

## Tuning — symptom → one flag

| What you see | Flag |
|---|---|
| a strip of desk or shadow left along an edge | `--trim-shave 2`, or `--edge-band 15` after a rectify |
| a real part of the page got cut off | `--no-trim`, or `--trim-step 12` |
| the page was shot at an angle and stayed a trapezoid | the quad was refused — `--rectify on` fails loudly and says why |
| rectification fired on something that is not a sheet | `--rectify off` |
| text too small or too large on the page | `--fit frame`, or `--fit content --margins L,R,T` |
| a pale stamp or a pencil note vanished | `--no-haze`, then `--paper-thr 95` |
| a coloured stamp came out grey | `--chroma 5`, or `--chroma-grow 10` |
| black print stayed brown or blue-ish | `--chroma 10`, or `--gray` |
| a handwritten page came out tilted | `--no-deskew` — the estimator reads text baselines, handwriting has none worth trusting |
| a near-square page came out sideways | `--rotate 0` |
| file too big | `--dpi 200`, `--quality 80`, `--gray` |
| an ordinary photo got bleached and straightened | it was taken for a document — `--photo on` |
| a document was treated as a photo and left untouched | `--photo off`, or lower `--photo-paper` |

## Speed

Per page, measured: ~30 s for a 12 MP phone photo on an M-series Mac, ~11–17 s
for a smaller one, ~98 s for the same 12 MP photo on a 4-core x86 VPS. Roughly
half of it is ImageMagick on full-resolution pixels; the two pure-Python passes
(quad detection, border flood) run on 400 and 520 px grids and cost under 3 s
together.

## Determinism

The same input gives the same bytes out on macOS/arm64, Linux/arm64 and
Linux/amd64 — verified by SHA-256 on a 12 MP HEIC. That is not free:
ImageMagick 7.1.1 and 7.1.2 swap the meaning of the `Divide_Dst` / `Divide_Src`
compose aliases, so a flat-field written with either name silently inverts on
the wrong build and the page comes out blank and speckled with a perfectly
normal-looking report. The operators are probed against two known pixels at
startup instead of being trusted.

## Limits

- **Rectification needs the sheet to stand out** — bright and low-chroma against
  a darker or coloured surface. White paper on a white desk does not separate:
  the quad is refused with a reason and the keystone stays.
- **Flat pages only.** A curved or crumpled page is not unwarped; that needs a
  3D model of the sheet. Shoot it flat.
- **No OCR.** The output is an image-only PDF. Run it through an OCR tool
  afterwards if you need selectable text.
- Pages are processed independently, so the scale can differ by a few tenths of
  a percent between pages of one document.
- The haze filter can erase a genuinely smooth light-grey fill (`--no-haze`).
- **Document-or-photo is decided on how much of the frame looks like paper**, so
  the two undecidable cases go the wrong way: a photo that is mostly a bright
  neutral surface (a white wall, snow) can still be treated as a document, and a
  document shot so small that it covers under 20% of the frame is kept as a
  photo — which is the safer of the two, since there is not enough resolution to
  scan it well anyway. `--photo on|off` settles it either way.

## Verify the result

The report can look perfectly sane while a page is ruined, so `--preview` writes
a PNG next to the output and it is worth a glance. Every bug found in this
pipeline so far produced a valid A4 PDF and a plausible report: a mask
composited at the wrong offset that erased most of the text, a bbox offset
silently reading `+0+0` that pushed every line off the right edge, a first line
of text mistaken for the sheet edge, and a flat-field inverted by the alias
above.

## License

MIT.
