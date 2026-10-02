# Corners by hand, a look per page, a page's geometry

This lets a person fix what the automatic run got wrong:
- drag the document's corners on the photo;
- choose how each page is cleaned;
- keep the edits made on a page (black boxes, a signature, the eraser, a
  crop) as long as the page's geometry has not changed.

Without the new fields and flags, everything runs as before, byte for byte.

## In the browser

```js
const r = process(
  [{ bytes, name,
     look: 'magic',                 // optional: auto | magic | color | original
     quad: Float64Array.of(...),    // optional: the corners set by hand
     kind: 'spread' }],             // optional: what the corners are
  ['--format', 'jpg', '--dpi', '200'], progress);

r.photos[j] = { kind, quads, width, height, page, hand }
r.pages[i]  = { jpg, dpi, geom: { sources, key, look, flat, lines, width, height } }
```

`look` and `quad` on a file apply to that file only. The same run may give
each photo its own.

## Corners set by hand

`quad` holds eight numbers per quad, x and y of each corner, in the photo's
pixels after its EXIF turn, as the browser shows the photo. The corners go
clockwise from the top left, as `detect()` returns them. Any order is
accepted: each quad is put clockwise from the corner nearest the top left.

| Numbers | `kind` | What it is |
|---|---|---|
| 8 | omitted, `sheet` | a sheet |
| 8 | `receipt` | a receipt: no face photo is looked for |
| 16 | omitted, `spread` | two facing pages, joined into one page at the fold |
| 8 or 16 | `cards` | one or two ID-1 cards |

The two pages of a spread may come in either order: the left page (or the
upper one) goes first.

A card's front and back shot as two photos are two files, each with its own
quad and `kind: 'cards'`. They are laid out on one page as before.

With corners set by hand:
- the search for the document is skipped;
- no page is turned by guess.

Everything after the search runs as usual:
- the band of desk cut off after the warp;
- the repaint of what leans in from outside;
- the finger;
- the tone and the cleaning.

Under `magic`, a sheet's sides between the corners may still follow its bowed
edges, as on an automatic run. The corners stay exactly where they were put.
Without this, nudging one corner of a sheet held bent in the hand would bend
it back. When the bowed edges are followed, `geom.flat` says `"edges"` and the
key changes.

"Back to automatic" is a run without `quad`.

## Looks

| `look` | What the page gets |
|---|---|
| none | as before: `--magic` or not |
| `auto` | the same as `--magic`: the magic paper on a sheet, the classic path on everything else |
| `magic` | the magic paper on any page, asked for by name |
| `color` | the classic path without the magic paper, what a run without `--magic` gives |
| `original` | the perspective and the A4 layout only |

`magic` on pages that the automatic run does not treat as a sheet:
- **A spread** gets the magic paper on both pages instead of the colour copy.
- **A photo with no document in it** is cleaned as a document.
- **A face photo on a sheet** keeps its colour, and the page around it
  turns white.
- **A card** is cleaned as paper. Its face photo keeps the card's colour copy.

`original` runs the same geometry as `color`:
- the warp;
- the band of desk cut away;
- on a photo with no sheet found, the border trim and the deskew.

Its pixels are left as they are:
- no tone, no whitening, no ink neutralized;
- nothing repainted: no desk, no finger;
- no sharpening.

On the command line: `--look auto|magic|color|original`, `--quad x,y,...`
and `--kind KIND`. `--quad` applies to every input.

## What was found: `r.photos`

There is one entry per raster: a photo is one, and each page of a PDF is one.

| Field | Meaning |
|---|---|
| `kind` | `sheet`, `receipt`, `spread`, `cards`, `photo` (no document), `none` (a document, but no quad found) |
| `quads` | `Float64Array`, eight numbers per quad, in the same pixels as `quad` above |
| `width`, `height` | the photo after its EXIF turn |
| `page` | the index of the page it went to |
| `hand` | the corners came by hand |

For the corner editor, send `quads` and `kind` back as they came. They then
give the same page and the same `key`. The array is `Float64Array` because a
`Float32Array` rounds the corners.

A sheet bent in the hand behaves differently under each look:
- **Under `auto` or `magic`**, it is one sheet: `kind: "sheet"` and its
  four outer corners.
- **Under `color`**, it is a spread of two pages, as it is processed.

## A page's geometry: `pages[i].geom`

| Field | Meaning |
|---|---|
| `sources` | the rasters the page was made from: one, or two for a pair of card photos |
| `key` | every decision that moves a photo's pixel onto the page, as one readable string |
| `look` | the cleaning the page got: `magic`, `color` or `original` (`auto` resolved) |
| `flat` | how the perspective came out: `quad`, `edges` (the sheet's bowed edges), `frame` (no quad), `cards`, `photo` |
| `lines` | the lines of text were straightened (magic) |
| `width`, `height` | the page in pixels |

The key strings together, in order:
- the quads (to 0.1 px, as the warp reads them) and the size they are warped to;
- the bows of a sheet's edges;
- the band cut off;
- a `--rotate` turn;
- the border trim;
- the deskew;
- the straightening field;
- the scale and offset on the page;
- the page's size and dpi.

**The contract.** Two runs of one photo with the same key put everything in
the same pixels, so the edits made on the page still fit it. A different key
means the geometry moved, and the edits no longer fit.

A change of look alone keeps the key, unless the page itself moved:
- `magic` followed a sheet's bowed edges (`flat: "edges"`);
- `magic` straightened the lines (`lines: true`);
- `magic` or `auto` took a sheet bent in the hand for one sheet where `color`
  sees a spread;
- `magic` turned a photo with no document in it into a document.

On the public examples and the local corpus of 51 photos:
- `color` and `original` gave the same key on every quad found;
- the corners sent back gave the same PDF, byte for byte, and the same key;
- `magic` changed the key exactly where it followed the edges or
  straightened the lines.

**One exception: `kind: "none"`.** On a photo where no quad was found, the
deskew angle and the fit by the text block are measured after the cleaning.
So on such a photo `original` can deskew differently, or not at all.

## A document's real size, and where it lies: `size`, `placed`, `place`

```js
r.pages[i].size    // { kind, mm: [w, h] | null, confidence, by, applied, candidates: [{ kind, mm, confidence }] }
r.pages[i].placed  // { x, y, w, h }: the document on the page, mm from its top left
r.pages[i].content // { x, y, w, h, px: [w, h] }: the same, and the document's pixels
r.pages[i].sheet   // { mm: [210, 297] | [297, 210] }
// the document where the person puts it: process again with
process([{ bytes, name, place: { w: 125, x: 42.5, y: 15 } }], args)   // mm
process([{ bytes, name, place: { size: 'real' } }], args)              // or 'fit'
```

**A photo holds no physical size.** The focal length and the sensor give an angle per
pixel, not the distance to the paper. So the size comes from what the document says of
itself.

- **`by: "mrz"`: its machine-readable zone.**
  - The zone is found by its geometry, without reading (`src/mrz.rs`): two lines of 44
    characters on a passport (TD3), two of 36 on a TD2, three of 30 on an ID-1 card.
  - Its pitch is fixed at 2.54 mm a character. So the zone is a ruler: millimetres per
    pixel, whatever the format.
  - One line alone counts too (the other cut off by the frame), at the document's foot.
  - A page the scanner leaves on its side is searched across as well.
  - A whole zone is 0.95 sure, one line at the foot 0.9.
  - `kind`, if within 8 % of a standard: `id3` (125×88), `id3-spread` (125×176), `td2`
    (105×74), `id1`.
- **`by: "card"`: an ID-1 card,** found by its rounded shape (the cards page, as before).
- **`by: "aspect"`: proportions only.** They give `candidates`, never a decision: A4, A5,
  A6, a passport's page and ID-2 all stand at 1.41–1.42. `candidates` always holds `id1`,
  `id3`, `a5` and `a6`, best fit first, for the person to choose from.
- **`by: "none"`:** a photo with no document.

**Laid at real size by itself (`applied: true`)** only by a zone (0.9 sure or more) or a
card. Then it goes on a portrait A4 (landscape when wider than 200 mm), in the middle
across, 15 mm from the top. Anything else is laid over the page as before. A passport
photographed close used to fill the A4; now it is 125 mm wide, as on a flatbed scanner.

**`place`, per file.**
- `{ size: "real" }` lays the document at the size found, or at the best candidate.
- `{ size: "fit" }` lays it over the page.
- `{ w, x?, y? }` lays it `w` mm wide (its height by its proportions), its top left at
  `(x, y)`. Without `x` and `y` it goes in the middle across, 15 mm from the top.

The page's key changes with it, as with corners set by hand. The command line has the same
as `--size {auto,real,fit}` and `--place W[,X,Y]`.

Measured on the local corpus, the examples and 20 more passport and card photos:
- the 10 passports whose zone shows are laid at real size (the internal passport's spread
  at 124×176 mm, nominal 125×176);
- no zone is found where there is none;
- every other page is byte for byte as before.

## Paper cleaned by hand: `cleanArea`

```js
const jpg = cleanArea(r.pages[i], mask);        // mask: w*h bytes (non-zero: clean) or RGBA (alpha)
const jpg = cleanArea(r.pages[i], mask, 90);    // the JPEG's quality, 88 by default
```

The site's brush or box. Under the mask, a grey shadow behind the print (a receipt's
fold, a desk at its edge) goes white and the print stays (`src/clean.rs`).

- **Strokes are told by their width, not their tone.**
  - The black-hat measures how much darker each pixel is than what lies round it within
    1.2 mm: the luma's closing less the luma.
  - A wide shadow scores nothing. A stroke on it scores what it stands out from it.
  - The threshold is the area's own noise (4 robust deviations, at least 4 % of white),
    not Otsu.
- **What goes, though it scores:**
  - parts of "ink" thicker than 1 mm (the rims of wide things);
  - anything on a coloured ground, or brown or orange itself: a desk, not paper. A blue
    pen or a red stamp keeps its colour.
- **Near-black stays whatever its width** (a logo, a black bar). No shadow on paper is that
  dark.
- **The ink kept is divided by the light round it** where that is paper, so a faded letter
  in the shadow comes back to its contrast. A pixel round each stroke is kept half.
  Everything else under the mask takes the paper's own tone, the brightest third of what
  lies round the mask (a cream sheet stays cream, a white one white). Nothing outside the
  mask changes.
- **What a fold bleached to the shadow's own tone is not in the pixels any more.** It is
  left white. Show the result at once, with undo.

**Measured** on a real receipt page, 200 dpi:
- the grey bands behind "Tel: +49 30 …", "Liefer-, und Leistungsdatum" and "Vielen Dank"
  went white, every letter whole;
- the dotted rules stayed;
- the brown desk under a second receipt went white;
- a near-black line along a receipt's edge stays.

The whole page takes about 0.35 s natively; a brushed strip, a fraction of that.

## Does a page look like a blank form? `looksLikeForm`

```js
const f = looksLikeForm(r.pages[i]);   // { jpg, dpi }: a page process() returned
// f = { empty, total, lines, rects, combs, boxes }
if (f.empty >= 6 && f.empty >= 0.6 * f.total) offerToFillIn();
```

It counts the page's fields, combs, check boxes and writing lines under 400 pt
(`total`), and the ones nothing is written in (`empty`), from the page's pixels alone. It
uses the geometry A4Norm Forms uses (`a4norm-geometry`, see a4norm-ocr's README), without
its OCR module. It adds 88 KB to `a4norm_bg.wasm` (1 514 KB → 1 604 KB, 29 KB gzip) and
takes about 70 ms a page in the one-thread module (87 ms at most on the plan's 50 pages). Nothing else changes: `process` runs as
before.

## A form's fields for filling it in by hand: `formGeometry`

```js
const g = formGeometry(r.pages[i]);   // { jpg, dpi, sizePt? }
// g = { sizePt, lines: [{id, x0, y0, x1, y1, empty}], rects: [{id, box, empty, label?}],
//       combs: [{id, box, cells, empty}], boxes: [{id, box, empty}], typicalFieldHeight }
```

This gives the page's writing lines, fields, combs and check boxes, in points from its top
left, for the free form mode. The fill module (`a4norm-fill`) then lays out the answers and
writes the PDF, without the OCR module.
- **The candidates** are the OCR module's `formGeometry`, with the same ids in reading
  order.
- **`empty`** says nothing is written in a candidate, measured as `looksLikeForm` does.
- **`label`** is the foot of a field's printed label ("Familienname" at the top of KG 1's
  frames). A value set in the field goes below it.
- **`sizePt`** is A4 when the page is A4-shaped, as the scanner makes it. Otherwise it is
  the page's pixels at `dpi`, unless it is given.

It adds 8.5 KB to `a4norm_bg.wasm` (3 KB gzip), and takes 60–130 ms a page.

## The command line: `--json PATH`

`--json PATH` writes the same `photos` and `pages[].geom` as JSON, beside
the PDF. With `--dry-run`, it writes them without the PDF.
