# `--magic`: paper cleaned like a phone scanner's "magic colour"

`--magic` is off by default. When it is on, a page recognised as a sheet of
paper gets a different cleaning:

- the shadows go;
- the paper comes out plain white;
- a sheet bent in the hand is flattened;
- a finger at its edge is painted out.

Everything else keeps the classic path: an open passport spread (colour
copy), an ID card, a photo, and any page on which a face photo is found. So a
caller can pass `--magic` on every scan. The binary decides per page whether
it applies, and the report says so.

## What happens on a sheet, in order

1. **Flattened by the sheet's own edges.** Each of the four edges is fitted as
   a gentle bow to the paper's outer outline:
   - the outline of the print inside is ignored;
   - the bite of a finger is trimmed as an outlier;
   - an edge that is not seen along all of its length, or that waves, counts
     as straight.

   A Coons patch over the four bows maps the sheet flat in one resampling. It
   is kept only when:
   - the lines of text come out no less level than with the plain four-corner
     quad, in both halves of the page;
   - it cuts no print along a side.

   Both checks run on 800 px copies. A white region "folded in the middle"
   that the spread detector found is treated as one sheet bent in the hand,
   never as two pages.
2. **A finger at the sheet's edge.** Skin-coloured, low-texture regions coming
   in from the edge are repainted in the paper's tone.
3. **The paper's own light divided out.** In each block of the page, a bright
   quantile gives the paper level. Blocks that are mostly print are dropped
   and filled from their neighbours. Unlike one heavy blur of the whole page,
   this follows a shadow with a sharp edge.
4. **Ink or paper.** A pixel is ink when one of these holds:
   - it is darker than the paper by more than the paper's own grain there,
     and an edge lies close to it;
   - it is part of a faint rule: dark along its length, light across it, at
     least 5% of the page long;
   - it is coloured: strongly, or moderately and darker than the paper with
     an edge near it.

   Solid dark areas are joined to the edged ink they touch. Faint rules with
   gaps are traced along their ridge, one pixel of slope at a time, and
   filled. They must keep off the page's margins, because a fold runs into
   them.

   The following go:
   - specks and small coloured crumbs (the back's print showing through);
   - thick shade touching the border or lying along a side (dark print on it
     stays);
   - grey blots by a side larger than a letter;
   - the sheet's own edge line and rim;
   - a fold's crease: a long, thin grey line running into a margin. Letters
     it crosses stay.

   Everything else becomes white, and ink is pressed a little darker
   (power 1.5).
5. **Bent lines of text straightened.** Text lines are found as long thin
   blobs of smeared ink. One smooth field (cubic across, quadratic down) is
   fitted to all of them. The field is applied only when all of these hold:
   - there are 10 or more lines;
   - they cover half the page;
   - the bend is under 5% of the page;
   - moving the line samples by the field leaves neither half of the page
     more bent. This is checked before any pixel moves.
6. **The page is laid out as usual.** The paper screen and the despeckle are
   skipped, since the paper is already white and the specks are gone.

## Changes outside `--magic`

A face photo on a sheet must now stand upright or square: width to height
0.5–1.3. A wide dark block, such as a shadow or a band of print, is no longer
taken for one and toned apart from the page. This changes pages with such a
false face photo, and only those. Nothing else in the default path changes
its output or its time.

## Where the time goes

Measured on 12 MP photos in Chromium, WASM, one thread, and in the threaded
build.

| Run | One thread | Threaded |
|---|---|---|
| First working version | ×2–3 | ×2–3 |
| Final, flat sheets | ×1.0–1.2 | ×1.2–1.5 |
| Final, a sheet held bent in the hand | ×1.36 | ×1.6 |

The bent sheet costs more partly because the classic path takes such a
photo for a spread and only makes a colour copy of it.

What made it fast:
- **Checks on small copies.** The curve and straightening checks run on
  800 px copies or on the line samples themselves, not on re-rendered pages.
- **Cells and running sums.** The paper grain is read on 4×4 cells. Openings
  and closings use running sums in row and column blocks, so their cost does
  not grow with the kernel.
- **Starting rule traces only where a rule can start.** A trace begins only
  where a long ridge follows, and runs in bands across threads.
- **One pass over the pieces of ink.** All the per-piece decisions share one
  labelling, and the rim search looks only at the strips along the sides.
- **Local finger repaint.** The finger search reads a box-averaged copy and
  repaints only the finger's own box.
- **No redundant finishing.** The paper screen and the despeckle are skipped
  after the magic paper.

With `--magic` on a card or a photo nothing extra runs. On a spread, only the
"is this a bent white sheet" test on the spread's report runs, which costs
nothing. On a sheet that turns out to carry a face photo, the edge fitting and
its check run (about 20–60 ms) before the face photo sends the page back to
the classic path.

`A4TIME=1` prints the time of each step (native builds).

## Limits

- **A sheet bent strongly in the hand, with few lines of text.** The
  straightening safeguards leave it bent rather than guess.
- **Print inside the deepest shadow.** Faint print there can be lost with the
  shade.
- **Faint rules broken into many short pieces.** They may stay dotted in
  places.
- **Printed rules that run into the page margin.** They may be taken for a
  fold and thinned.
