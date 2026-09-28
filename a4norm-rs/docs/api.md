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

## The command line: `--json PATH`

`--json PATH` writes the same `photos` and `pages[].geom` as JSON, beside
the PDF. With `--dry-run`, it writes them without the PDF.
