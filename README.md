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
driving ImageMagick and poppler, and the container is 104 MB. A second image,
`:full`, adds one segmentation model for the photographs the brightness rule
cannot solve — see **Two images** below. The light image is unchanged by it.

**Try it without installing anything:** [@a4norm_bot](https://t.me/a4norm_bot)
on Telegram is this tool behind a chat window — send a photo, or a whole album,
and the A4 PDF comes back. It runs the same container as below.

```bash
docker run --rm -v "$PWD:/work" ghcr.io/georg-malahov/a4norm:latest \
  -o /work/contract.pdf /work/page1.HEIC /work/page2.HEIC /work/page3.HEIC
```

Four test images live in `examples/`, and none carries anybody's data.
`sample-photo.jpg` is a generated letter, degraded to look photographed (warm
cast, uneven light, a tilt, a desk border). `notebook-photo.jpg` is a real
phone shot of a handwritten to-do list on a spiral notebook, at an angle on a
wooden desk — the case that exercises the harder half of the pipeline: a quad
that is nearly square, faint pencil, and a binding whose rings sit inside the
page's own margin. `landing-notebook.webp` and `landing-invoice.webp` are the
demo photos the product page shows (a notebook next to a laptop, a synthetic
invoice with a hard shadow over its corner). They are tested too because the
landing notebook once came out with a band of desk above the page while the
README's notebook was fine, and nothing noticed: whatever the product page
shows is now under the same test. A one-line smoke test:

```bash
docker run --rm -v "$PWD:/work" ghcr.io/georg-malahov/a4norm:latest \
  --preview /work/examples/sample-photo.jpg
```

The regression corpus is larger than this — 26 pages covering receipts, forms,
multi-page PDFs and sheets darker than their background — and it is
deliberately **not** in this repository: those are real documents belonging to
real people. What it protects is stated in the commit messages that changed
behaviour, with the numbers each decision turned on, so a change can be argued
about even by someone who cannot run it.

The two public examples ARE under test, in CI, on every push and pull
request, inside both images before either is pushed: `tests/regression.py`
checks what a4norm reports (the notebook's quad is rectified, its left side
is judged a binding and cut while the other three are kept, the page turns
and the picture does not) and what the page looks like (exact A4, not blank,
lines run across, the blue signature still blue, the pencil still grey),
plus a tolerant comparison with a low-resolution golden render. Deploy waits
for it. Run it locally with `tests/run-in-docker.sh`; when a change is meant
to alter the pages, refresh the goldens with `--update-goldens` in each image
and look at them before committing.

The private corpus has a runner of its own, `tests/corpus.py`, which reads
`tests/corpus/` — photos plus a `cases.json` of what each must come out as
(spread found, turned by how much, face photo kept and in the lower left,
text running across). That directory is in `.gitignore` and `.dockerignore`
and exists only on the machines that run it; the runner and its format are
public, the documents are not. A photo the tool cannot handle yet stays in
the corpus marked `known_fail` with the reason, is reported as `xfail`, and
flips to `XPASS` the day it starts passing. Every run writes
`tests/corpus/out/sheet.png`, each input beside the page it became — the
checks are necessary, never sufficient.

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
| `POST /pages` | a PDF this service made → its page images, as JSON |

Send images as `multipart/form-data` (any field names, repeat for several pages,
page order = part order), as a raw body with an image content type for one
image, or as JSON — `{"images": ["<base64>", "<base64>"]}` — which is the shape
to use from an automation platform, where the number of multipart fields is
fixed at design time and "the user sent five photos" has no natural form. Any `a4norm` flag can be passed as a query parameter (`?dpi=200&gray=1`,
underscores allowed: `?paper_thr=85`). The response carries `X-Pages` and
`X-Seconds`.

```bash
curl -X POST http://localhost:8080/scan \
  -F p1=@page1.HEIC -F p2=@page2.HEIC -o document.pdf
```

`/pages` is the cheap way back to images. a4norm writes one JPEG per page, so
the pages of its own PDF are already sitting there as streams and can be copied
out without re-encoding — milliseconds, against about a minute per page to scan
them again. Send the PDF as a raw body or as JSON `{"pdf": "<base64>"}`; the
answer is `{"count": N, "types": ["jpg", …], "pages": ["<base64>", …]}` in page
order. A PDF of vector text has nothing embedded to copy and comes back `422`.
The route does no image processing, so it does not take a concurrency slot and
never queues behind a scan.

```bash
curl -X POST http://localhost:8080/pages \
  -H 'Content-Type: application/pdf' --data-binary @document.pdf
```

Concurrency is capped at one page at a time by default (`--max-concurrency`):
a page is tens of seconds of CPU, so an unbounded server is a denial-of-service
switch. Bodies over 40 MB are refused, a queued request waits 120 s for a slot
before `503`, and a job is killed after 600 s.

## Two images

`ghcr.io/georg-malahov/a4norm:latest` is the tool: Alpine, 104 MB, stdlib-only
Python over ImageMagick and poppler. Nothing about it changed.

`ghcr.io/georg-malahov/a4norm:full` is the same a4norm plus `a4norm-seg`, a
one-file helper that runs U^2-Net through onnxruntime. It exists for one
failure the brightness rule cannot be tuned out of: **the sheet being darker
than what it lies on**. Measured on real photographs, a grey thermal receipt on
a marble counter reads as 0–3% paper-like where the true figure is 30–45%, so
it was classed as a photograph and passed through untouched; a white evacuation
sign on a white wall left the wall in the output. With segmentation all of them
rectify.

There is no flag. The model is consulted **only after** the brightness detector
has already failed, so a page that is the brightest thing in frame never pays
for it, and the light image behaves identically to the full one on every input
where brightness succeeds. If you want the tool without a model, run the light
image — that is what the two tags are for.

What the model returns is not trusted on sight. It answers "what stands out
here", which is not the same question: on a synthetic letter it picked out the
signature (0.2% of the frame), and on a photo of a desk it outlined the
monitor, which really is a bright quadrilateral and passes every geometric
test. So its answer goes through the same acceptance tests as the brightness
one, plus a last check the geometry cannot make — whether the inside of the
region reads as paper, by the same bright/low-chroma rule measured against the
region's own paper level. Measured: 60–97% for every real sheet here, 16% for
the desk. `--dry-run` prints which path was taken and why.

Debian, not Alpine, because onnxruntime ships no musl wheels. The model is
baked in at build time rather than fetched on first run: the service is meant
to run read-only with a tmpfs, where a runtime download either fails or repeats
after every restart. Nothing is downloaded at runtime and nothing phones home.

The weights are U^2-Net under Apache-2.0 and are redistributed under that
licence; `LICENSE-THIRD-PARTY` carries the notice. Only the `u2net` checkpoint
is shipped and it is pinned by name: several other background-removal
checkpoints in common use (isnet-*, bria-rmbg, u2net_portrait) are licensed for
non-commercial use only, and an automatic fallback to one of them would quietly
change what this image may be used for.

## What it does, in order

1. **Rasterize** — a PDF's embedded image is extracted rather than re-rendered;
   rendering applies the ICC profile and flattens the tonal range.
2. **Rectify** — the sheet is the big bright low-chroma region, both tests
   relative to the image's own paper level, so a dim photo works like a bright
   one. When that assumption is simply false — a grey till slip on a marble
   counter, a white sign on a white wall, where the background really is
   brighter than the paper — the `:full` image asks a segmentation model
   instead, and judges its answer by the same tests (see **Two images**). Its quadrilateral is warped flat. A quad is accepted only if it looks
   like a sheet (15–90% of the frame, filling ≥80% of its hull, corners 45–135°,
   opposite sides within 1.8×); otherwise the reason is printed and the rest of
   the pipeline carries on, because rectifying on a wrong quad is worse than not
   rectifying.
   **An open booklet is looked for first** — a passport photographed with both
   pages showing (`--spread`, auto). It breaks the one-sheet model three ways:
   the red perforation strip at a passport's spine (chroma 75 against the
   paper limit of 45) splits the paper mask into two sheets, and the larger
   one used to be rectified alone while its facing page was thrown away; the
   pages curve into the spine, so the outline is a flat "V" at the fold that
   no single homography flattens; and the hand holding it open bites into
   the edge. So two facing pages are found (two paper regions of similar
   size, or one region whose long edges both bend or step in the middle —
   an international passport has no strip between its pages), each page edge
   is fitted as a *support line* — the straight line the most edge points lie
   on with no paper beyond it, which a thumb's curved outline cannot win —
   and the fold is where the two pages' edges meet. When a hand hid nearly
   all of an edge, the facing page's edge is used for both. Each page is
   warped to one common size and the two are joined at the fold; a finger
   still lying over the outer edge is repainted in its page's own tone (skin
   and the red strip are the same colour to within a few units, so the band
   around the spine is left alone; "saturated" is measured against the pages'
   own median, because under warm light a whole page reads as skin — one
   flood repainted 36% of a spread — and a "finger" over 6% of the spread is
   taken for the page's colour and left alone). Passport pages are not white paper, so
   when the strict paper mask finds no spread two looser readings get a say:
   chroma up to 100 (salmon-pink pages) and Otsu's brightness split (a
   booklet half in its own shadow). Then the spread is **turned upright from
   the page itself**: which way the text runs (ink in long runs across the
   lines against along them — 0.26 as photographed, 3.79 turned), and which
   way is up from where the face photo sits — on the left of its page, on a
   Russian passport's page 3 and on every ICAO data page. With no photo to go
   on it says so, and `--rotate` settles it.
   **An identity card or a driving licence** (`--cards`, auto) is looked for
   alongside. ISO/IEC 7810 ID-1 — German Personalausweis, Russian and EU
   driving licences, bank cards — is 85.60 × 53.98 mm, 1.586:1, where A4 is
   1.414 and a passport page 1.42, and that proportion is what tells a card
   from a sheet. The same edge fitting finds one or two card-shaped regions
   (60–120° corners, 1.50–1.68 measured through perspective). Two regions
   touching along a side are a booklet's pages and are left to the spread
   detector. A lone card never beats a spread unless it covers 70% of it:
   a passport page can pass for a card, a card is never two pages.

   A **frame edge is no edge**: where a region runs into the photo's border,
   that stretch of its outline is dropped. A glare over a card that reached
   the top of the photo gave it a perfectly straight "top edge" along the
   frame. For the same reason a support line is penalised 1 per point of
   paper outside it, not 3: at 3 a glare spur standing above a card beat
   the card's real edge (24% support against 70%).

   Each card is rectified straight to its **real size** at `--dpi` and turned
   so its face photo is on the left. There is no search for the photo: it is
   only ever in the left third of a front, so that place and its mirror are
   measured. A place holds a photo when its cells are dark against the paper
   near them AND dark in unbroken columns. Text has gaps between its lines;
   a portrait runs from hair to collar. Fronts measured 0.38–0.62 dark and
   0.20–0.60 in columns, backs 0.14 and 0.00. A finger at the edge is
   repainted first.

   The card is toned as a **colour copy, not a scan**: the tint and the
   security print are the document. The light is evened, the same factor on
   every channel, and nothing is whitened. It gets its rounded 3.18 mm
   corners and a hairline edge so a white card still has one on a white
   page. Cards are laid out **front above back on one A4**, whether both are
   in one photo or in two photos in a row (`--card-size fit` blows them up
   to the page width). What makes a card is its shape, not a face: a bank
   card has none, and a German ID card's pale portrait once read as "no
   face" on both sides of a perfectly shot pair, which went out as a
   snapshot and a scanned page. The face only decides which side goes on
   top and which way up — found as a dark, columnar block in the left third,
   or as a place far darker than its mirror (0.41 against 0.06 on that ID).
   A card beats a spread only when the spread was one region cut at a kink:
   a whole open passport is card-shaped too. `--cards off` for a 16:10
   rectangle that is no card.
   **When brightness cannot see the document, its edges can** (`--edges`,
   auto). Every detector above looks for PAPER, and a white page on a white
   desk, a passport over a light floor or a page whose colour pictures broke
   the paper mask is not brighter than its surroundings. It still has an
   edge: a long straight step in brightness or colour where it ends. So the
   frame is also read as lines — Canny edges of the brightness AND of the
   saturation (a lilac card on white paint barely steps in brightness), a
   Hough transform — and every pair of near-horizontal lines with every pair
   of near-vertical ones is a candidate outline, scored by how much of it is
   actually edge (every side at least 45%). The outline is only believed
   where brightness failed: it found nothing, a scrap inside the outline
   (30% smaller or more), or the frame itself (70% of it and more). A
   "scrap" whose own outline is edged on three sides of four is no scrap
   but a sheet, and it is kept unless a fold makes the larger outline a
   spread: the landing's notebook page (sides 0.40, 1.00, 0.96, 1.00 edge)
   once lost to an outline around it and the desk above it.

   - A card-shaped outline (1.50–1.68) goes to the card path, which still
     lays it out as a card, face or no face.
   - An outline with a **fold** becomes a spread. The fold is a line across
     the middle, parallel to the short sides, and it must stand alone: a card
     or a page of print has one under every row of text (6–14 measured),
     where a spread has exactly the two edges of its red strip, within 3% of
     each other. Rivals are counted relative to the best line, because the
     two ImageMagick builds find different numbers of weak ones.
   - Otherwise a sheet — only with 60% of its outline edge, 15–85% of the
     frame (a sheet filling it is the border trim's job: vignetting draws
     lines too) and shaped like one (1.25–1.6: a square of railings around
     a card in a hand once cut the card in half).

   A spread's orientation now looks for the face photo **in its place** —
   the left third of the lower page — on each candidate turn, as on a card,
   instead of anywhere; the place that is dark with a portrait wins, and a
   light portrait (dark 0.32 against 0.02 on the other turn) wins on
   contrast. The photo kept out of the paper treatment is looked for in
   that place too (shape 0.45–1.8, three passes from strict to loose), and
   its box is grown to a passport photo's height (45 mm of the page's 125),
   because the dark cells are the hair and a light chin below them came out
   white.
3. **Or decide there is no document at all.** If no sheet quad was accepted
   *and* the paper-like area is under `--photo-paper` (20%), the frame is a
   photo, not a page. This is decided BEFORE anything touches the pixels, and
   the photo path is a single early return, not this pipeline with stages
   switched off, so nothing written below can leak onto it. The only things
   that happen to the picture are the geometric fit onto the page and the JPEG
   encode at `--photo-dpi` (200) and `--photo-quality` (82) with 4:2:0 chroma —
   text needs full chroma and 300 dpi, a snapshot does not. No flat-field, no
   tone, no haze filter, no paper-whitening, no ink neutralisation, no sharpen;
   `--gray` does not apply either. A landscape photo turns the page instead of
   being rotated. Measured paper-like area: 9% for a photo of a screen, 40–83%
   for every real document and test page here, so the threshold sits in a wide
   gap. `--photo off` forces the scanner treatment, `--photo on` forces the
   short path — and, being a real early return now, no longer rectifies on the
   way.
4. **Erase what leans in from outside the sheet** — non-paper *connected to the
   frame edge* is desk, shadow or binding; sheet content cannot reach the border.
   It is flooded from the border and repainted in the page's own paper tone, and
   a thick band of it (a binding) is cropped away. Connectivity alone is not
   enough, though: a printed form's grey shaded panel that reaches the edge of
   the sheet is *also* darker than the paper and *also* touching the border, so
   it used to be flooded and then guillotined together with the print sitting on
   it. Each side of the flood is therefore judged before it is acted on. A side
   whose flooded pixels carry printed marks (above `--band-structure`, 6%) or
   are only mildly darker than the paper (brighter than `--band-dark`, 60% of
   the page's own paper level) is part of the DOCUMENT: it is never cropped, and
   the flood there is pulled back to a shallow `--edge-keep` strip (1% of the
   short side). Measured: a real spiral binding 41% of paper / 4.6% structure, a
   synthetic one 26% / 2.8%, a shaded form panel 73–77% / 5.1–8.6%. When the two
   tests disagree the content wins — a slightly dirty edge is a far better
   failure than amputated text.

   A binding still gets through that, because a ring carries a bright specular
   glint and the dark rim around it reads as a mark on a paper-bright bed. A
   notebook page came back with a column of black squares down its margin on
   exactly that — and there is no bar to put it under: the rings measured 11.3%
   structure on one photograph and 18.2% on a sharper one of the same notebook,
   where the BOTTOM edge, which is handwriting and must be kept, measured 18.5%.
   Darkness does not settle it either; a real form's top band is just as dark
   (40% of the paper level) and is nothing but print.

   So the PATTERN decides. A spiral binding is a row of equal blobs at an equal
   pitch, and nothing a printer puts on a page is: measured across the corpus, a
   binding's ring and gap lengths vary by 0.06–0.26 over 17–34 rings covering
   half the side, while every printed edge that had to be kept reads 0.41 and up
   or has one or two blobs and no pitch at all. The test only overrides a
   DOCUMENT verdict, and only for a band darker than `--band-dark`, so it can
   take a side away from the content but never give one to it.
5. **Keep a face photo out of the paper treatment.** Everything from here on
   turns light smooth areas into white paper, and a face is a light smooth
   area: a passport came back with white holes for cheeks. A photo is a
   compact block of cells mostly darker than the paper *near them* (the paper
   level is a closing over the cell grid, so a page half in shadow does not
   read as one big photo), of a portrait's size and proportions, filling at
   least 40% of its box — hair and clothes, the lighter face between. It is
   cut out before the flat-field, toned on its own (black at its darkest, white
   at its own light background, grey when it is black-and-white) and laid back
   over the finished page with a feathered edge. `--no-keep-photo` turns it
   off. Neither public example has one.
6. **Flat-field** — divide by a smoothed background estimate, in colour, which
   both evens the light and white-balances the paper. The estimate is built on
   a point-sampled copy with the kernel scaled to match, because it is squeezed
   to 6% and blurred there anyway: six times cheaper, and within RMSE 0.01 of
   the full-resolution estimate.
7. **Deskew** above 0.4°, under 5°, after the flat-field (before it, a dim photo
   binarizes into one blob and a real tilt measures as 0.0°). Skipped after a
   rectify, which already set the orientation.
8. **Neutralize the ink** — a photo tints black print warm. Everything goes
   neutral except pixels that are both high-chroma and dark: real coloured ink,
   any hue.
9. **Tone** by histogram percentiles, then erase bright featureless haze (a soft
   shadow or a finger goes; anything with structure survives), then clean the
   paper to pure white with a 1 px guard ring around every glyph.
10. **Fit to A4** — the PAGE turns, never the picture. A wide result is laid on
   a landscape A4; `--rotate auto` rotates nothing at all — except a spread,
   which is turned from the evidence on its own pages (step 2). Turning the pixels
   instead assumes a wide frame means a sideways sheet, and it usually does not:
   a square notebook page shot in a wide frame is wide because of the FRAME, and
   standing its lines on end makes it unreadable. A portrait sheet genuinely
   photographed sideways stays sideways, which the reader fixes with one
   keypress. `--rotate 90/180/270` still turns the picture, and the page follows
   it; `--landscape` forces a landscape page whatever the shape. Scale comes
   from the real sheet edges when two opposite ones are visible
   (exact px-per-mm, no assumption about the layout). Otherwise the frame, which
   at least fills the page. The ink-block fit, which infers the scale from an
   assumed text width, is used only when NO sheet edge was found at all — that
   is the one case where the margins genuinely have to be guessed, and guessing
   them when the sheet is plainly inside the frame shrinks a wide worksheet to
   two thirds of the page (measured 2.11 against the frame's 2.69 and the real
   sheet's 3.15 on the same photograph). `--fit content` still forces it.

   A page is only as sharp as the photo it came from: when the photo holds
   less than 180 dpi of real detail at the size it lands, the page is written
   at 200 dpi with 4:2:0 chroma instead of 300 — a passport spread from a
   1280×960 snapshot went from 1.6 MB to 577 KB with no difference visible
   side by side. The report says so; an explicit `--dpi` is always obeyed.
11. **Clean the open paper.** A white page with grey grain in one corner
    looks worse than the photo it came from. It is where a hard shadow lay
    over the sheet's corner and the flat-field could not follow its edge, and
    the same goes for dust and a pencil tip's fleck. A mark is erased only
    when three things hold:
    - it is small (under 2.5 mm square);
    - it is light (nothing in it darker than 60%: grey 8 pt print measures
      40–60%, shadow grain 77% and lighter);
    - nothing of substance lies within 3 mm of it. Substance is a larger mark,
      a long thin one (a light ruled line breaks into speck-sized fragments),
      or a few dark pixels.

    A full stop, an i's dot and a decimal point all sit next to print and
    stay. A cluster of grain has no print among it and goes whole. A large
    mark filling a corner of the page and running into its edge is taken for
    that shadow in one piece: it goes too, except the reach around any print
    it covers. The marks are measured at 150 dpi, the erasing is done at full
    resolution, and it costs about 1.5–2 s a page. `--no-despeckle` turns it
    off.

`--dry-run` prints which path each page took and why. Every parameter above is a
flag; `--help` lists them.

## Tuning — symptom → one flag

| What you see | Flag |
|---|---|
| a strip of desk or shadow left along an edge | `--trim-shave 2`, or `--edge-band 15` after a rectify |
| a real part of the page got cut off | `--no-trim`, or `--trim-step 12` |
| a form's shaded panel got erased, or its left column cut off | it was judged desk — `--band-dark 75`, or `--band-structure 3` |
| a shaded panel survived but its outermost few mm went white | `--edge-keep 0.5` |
| a binding or a dark desk band stayed after a rectify | it was judged document — `--band-dark 45`, or `--band-structure 10` |
| the page was shot at an angle and stayed a trapezoid | the quad was refused — `--rectify on` fails loudly and says why |
| rectification fired on something that is not a sheet | `--rectify off` |
| text too small or too large on the page | `--fit frame`, or `--fit content --margins L,R,T` |
| the page came out small, adrift in wide empty margins | the layout was guessed — `--fit frame`, or `--fit edges` |
| one page of a set came out smaller than the others | its sheet edges were not all found; compare the `border cut` lines in `--dry-run` |
| a pale stamp or a pencil note vanished | `--no-haze`, then `--paper-thr 95` |
| tiny light marks on open paper vanished (faint dots, a light dotted line) | `--no-despeckle` |
| grey grain left in a corner of an otherwise white page | its flecks were dark (under 60%) or near print — `--dry-run` shows what `cleaned` took |
| a coloured stamp came out grey | `--chroma 5`, or `--chroma-grow 10` |
| black print stayed brown or blue-ish | `--chroma 10`, or `--gray` |
| a handwritten page came out tilted | `--no-deskew` — the estimator reads text baselines, handwriting has none worth trusting |
| a wide page landed on a landscape sheet and you wanted portrait | `--rotate 90` — it turns the picture, and the page follows |
| a spiral binding survived as dark marks in the margin | its rings were not regular enough to be recognised — `--band-dark 45` to judge the band on darkness alone |
| a regular row of printed marks at one edge got cut as a binding | `--band-dark 75`, or `--edge-keep 8` to keep the band |
| file too big | `--dpi 200`, `--quality 80`, `--gray` |
| a small photo came out at 200 dpi and you need 300 | `--dpi 300` — an explicit value is always obeyed |
| a passport spread was rectified as one page, or only one page of it kept | `--spread on` fails loudly with each paper mask's reason |
| something that is not a booklet was split in two and joined | `--spread off` |
| a spread came out upside down | no face photo told up from down — `--rotate 180` |
| a face photo came out bleached | it was not found — the report has no `face photo at` line |
| an ID card came out as a scanned page, not a card | not card-shaped — the report has no `card:` line |
| a card's back landed on top | neither side's face photo was found; the photos keep their input order — shoot the front first |
| a card's front and back landed on two pages | they were not two photos in a row, or one of them was not found as a card |
| a card came out upside down | its face photo was not found — it was taken for a back |
| cards too small to read | `--card-size fit` — each card at the page width |
| something that is not a card was laid out as one | `--cards off` |
| the page was cropped to a wrong rectangle "found by its edges" | `--edges off` |
| a dark picture or logo on a page kept a grey box around it | it was taken for a face photo — `--no-keep-photo` |
| an ordinary photo got bleached and straightened | it was taken for a document — `--photo on` |
| a photo came out in colour although `--gray` was given | the photo path is a passthrough; `--gray` is a document flag |
| a document was treated as a photo and left untouched | `--photo off`, or lower `--photo-paper` |
| a sheet darker than its background was ignored entirely | the brightness rule cannot see it — use the `:full` image |
| the `:full` image rectified something that is not paper | `--dry-run` shows the paper share it measured; the gate is 40% |

## Speed

Per page, measured in the 2-CPU x86 container the service actually runs in:

| input | before | after |
|---|---|---|
| 12 MP phone photo of a form | 105 s | **32 s** |
| 12 MP photo, pencil on white | 152 s | **36 s** |
| 5 MP photo of a printed form | 66 s | **24 s** |
| supermarket receipt (needs the model) | 45 s | **18 s** |
| 1.2 MP notebook page | 34 s | **18 s** |
| eight-photo set, total | 545 s | **190 s** |

The two halves of that are independent, and only one of them is code:

| | 12 MP + 5 MP + receipt + notebook page |
|---|---|
| before | 249 s |
| `MAGICK_THREAD_LIMIT` alone | 193 s |
| script alone | 129 s |
| both | 94 s |

More cores do help, but far less than their count suggests — and only if both
variables move together, because `OMP_NUM_THREADS` caps the pool ImageMagick
draws from:

| service CPUs | thread pool | same four photos |
|---|---|---|
| 2 | 2 | 92 s |
| 4 | 2 | 94 s — the extra CPUs sat idle |
| 4 | 3 | 80 s |
| 4 | 4 | 76 s |

Twice the cores buys 1.21x. An earlier note here blamed that on a 65% serial
fraction and named the two pure-Python passes and the process launches as the
cause. Both were wrong, and the numbers are worth keeping because they are the
ones that stop anyone optimising the wrong end of this:

| | share of a 12 MP page |
|---|---|
| pure Python (quad detection, border flood) | **2%** |
| the 19 `magick` launches (2 ms each) | 0.4% |
| everything else: ImageMagick on pixels | ~98% |

And most of those pixels do thread: measured 1t→4t on an idle machine, the
marks map gets 3.3x, the flat-field 2.0x, the output resize 1.8x, the warp
1.6x. So the 1.21x is mostly the host, not Amdahl — four cores shared with
nineteen other services are not four free cores. The service stays at two
CPUs for that reason: 17% off a 32-second page is not worth a scan taking the
whole box while n8n is trying to answer the same user. Both numbers are one
line each if that trade ever changes.

The thread limit is one line in `Dockerfile.full`, and the note there explains
why ImageMagick was running single-threaded on two cores. The script side was
almost entirely *fewer passes over full-resolution pixels*, not cleverer maths:

- every intermediate PNG is written with `png:compression-level=0`. They are
  thrown away one stage later, and compressing them was **27%** of the whole
  run — one line, and the output PDF is byte-for-byte identical either way.
- ink neutralisation went from six processes to one, tone/haze/whitening from
  four to one, and the resize and the A4 layout from two to one. Each of those
  handoffs was a full-resolution PNG written and read back; `mpr:` registers
  keep the page in memory instead.
- the background estimate is built on a point-sampled copy with the kernel
  scaled to match. It gets squeezed to 6% and blurred there regardless, so
  six times the work bought RMSE 0.01.
- the perspective warp never renders more pixels than one page holds. The fit
  was scaling them straight back down.
- `-auto-orient` is skipped when the EXIF already says `TopLeft`, instead of
  rewriting a 12 MP photo to apply nothing.

- the border marks map grows its disk by iterating a diamond and a square
  instead of convolving a dense one. Byte-identical output, every border
  decision unchanged, and 1.9x on builds without a vectorised dense path —
  though only 1.3x on the one the service runs, which is why the service saw
  almost none of it.

The two pure-Python passes (quad detection, border flood) run on 400 and 520 px
grids and are untouched by all of this — correctly, as the table above shows.

Nothing here is free: against the old pipeline the pages differ by RMSE
0.003–0.032, worst on faint pencil, and the ink-colour mask keeps 8.67% of a
biro-covered page instead of 9.02%.

### Measured and rejected

Kept here because each one looks like an obvious win until it is measured, and
three of the four cost real content:

| idea | what it bought | why not |
|---|---|---|
| haze mask at half resolution | 0.3 s | erased printed rules and digits |
| whole chroma mask at a third | 0.8 s | dropped 42% of the coloured-ink area |
| marks map on a downscaled copy | 1.4 s | a document edge reading 35.5% structure read 0.0% and was guillotined — at every filter and every factor, down to 1/√2 |
| marks map with the erode half dropped | 1.5 s | leaves the whole spiral binding standing on the page |
| tone curve from a sampled histogram | 13 ms/page | `-contrast-stretch` costs 1–30 ms on a normal page. The 850 ms that made this look worthwhile was ONE pathological page out of 21; the replacement is slower on most of them |

## Determinism

The same input gives the same bytes out on macOS/arm64, Linux/arm64 and
Linux/amd64 — verified by SHA-256 on a 12 MP HEIC, and the thread count does
not enter into it either (identical at 1, 2, 4 and 8). **Within one image.**
The two images do not agree with each other: Alpine builds ImageMagick with
HDRI and Debian does not, so the `:full` image clamps intermediate values the
`:latest` image keeps, and the same photo comes out a few least-significant
bits apart. Verified both ways — the light image gives the same SHA-256 on
arm64 and amd64, so it is the build and not the architecture. Nothing visible
rides on it; it is only worth knowing before diffing one image against the
other. That is not free either:
ImageMagick 7.1.1 and 7.1.2 swap the meaning of the `Divide_Dst` / `Divide_Src`
compose aliases, so a flat-field written with either name silently inverts on
the wrong build and the page comes out blank and speckled with a perfectly
normal-looking report. The operators are probed against two known pixels at
startup instead of being trusted.

## Limits

- **Rectification needs the sheet to stand out** — bright and low-chroma against
  a darker or coloured surface. White paper on a white desk does not separate:
  the quad is refused with a reason and the keystone stays.
- **A spread needs its pages to stand out too.** White pages held over a white
  desk join the desk in every paper mask, and a passport cover's dark rim is
  too thin to enclose them; the spread is not found. The same goes for a
  booklet whose pages are hidden by the hand more than they are shown.
- **A card must stand out from what it lies on.** A licence on a white car
  roof, on light wood or in a hand over a bright staircase joins the
  background in every paper mask and is scanned as a document instead.
  Edge-based finding is the next step for both cards and spreads.
- **A card's back has no face photo**, so it keeps the orientation it was
  shot in; shot upside down, it stays upside down.
- **Up from down on a spread comes from the face photo.** Pages without one —
  a passport's registration pages — are turned so the text runs across, but
  may come out upside down; the report says when it had nothing to go on.
- **Flat pages only.** A curved or crumpled page is not unwarped; that needs a
  3D model of the sheet. Shoot it flat.
- **No OCR.** The output is an image-only PDF. Run it through an OCR tool
  afterwards if you need selectable text.
- Pages are processed independently, so the scale can differ by a few tenths of
  a percent between pages of one document.
- The haze filter can erase a genuinely smooth light-grey fill (`--no-haze`).
- **The photo decision is only consulted when no sheet quad was accepted.** A
  snapshot whose subject happens to look like a sheet — a whiteboard, a lit
  screen, a bright rectangular panel — is rectified and scanned however small
  its paper-like area is, and `--photo-paper` never gets a vote. That is the
  remaining way a real photograph can take the document path; `--photo on`
  settles it.
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
