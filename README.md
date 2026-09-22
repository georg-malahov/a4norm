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
5. **Flat-field** — divide by a smoothed background estimate, in colour, which
   both evens the light and white-balances the paper. The estimate is built on
   a point-sampled copy with the kernel scaled to match, because it is squeezed
   to 6% and blurred there anyway: six times cheaper, and within RMSE 0.01 of
   the full-resolution estimate.
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
   (exact px-per-mm, no assumption about the layout). Otherwise the frame, which
   at least fills the page. The ink-block fit, which infers the scale from an
   assumed text width, is used only when NO sheet edge was found at all — that
   is the one case where the margins genuinely have to be guessed, and guessing
   them when the sheet is plainly inside the frame shrinks a wide worksheet to
   two thirds of the page (measured 2.11 against the frame's 2.69 and the real
   sheet's 3.15 on the same photograph). `--fit content` still forces it.

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
| a coloured stamp came out grey | `--chroma 5`, or `--chroma-grow 10` |
| black print stayed brown or blue-ish | `--chroma 10`, or `--gray` |
| a handwritten page came out tilted | `--no-deskew` — the estimator reads text baselines, handwriting has none worth trusting |
| a near-square page came out sideways | `--rotate 0` |
| file too big | `--dpi 200`, `--quality 80`, `--gray` |
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
