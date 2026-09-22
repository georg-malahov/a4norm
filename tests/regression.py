#!/usr/bin/env python3
"""Regression test over the public examples in examples/.

Runs the a4norm that is installed where this runs -- in CI that is the one
baked into the image about to be pushed, so what is tested is what ships --
and checks each page against two kinds of expectation:

1. STRUCTURAL facts, read from a4norm's own per-page report and from the PDF:
   one page, an exact A4 media box, which path the page took (rectified or
   not), which border sides were judged document and which were cut as a
   binding, which way the page faces, and pixel facts that say what the page
   must look like (not blank, text lines run across the page, blue ink still
   blue, pencil still grey, no binding left in the margin). These are the
   contract. They survive an ImageMagick upgrade; they do not survive a change
   that breaks the demo.

2. A tolerant IMAGE comparison against a committed low-resolution golden
   (tests/golden/<flavor>/*.png): mean absolute error and the share of pixels that
   changed a lot. This is the net for "something visibly changed that no
   structural fact names". It is deliberately NOT a byte or hash comparison:
   the light image (Alpine, HDRI ImageMagick) and the full image (Debian,
   non-HDRI) already disagree on the same photo, and any change of the
   ImageMagick build can move the pixels again without anything visible
   changing. See TOLERANCE below for the numbers the thresholds were set from.

Stdlib only, so it runs inside the light image as-is:

    python3 tests/regression.py                    # test
    python3 tests/regression.py --update-goldens   # rewrite this flavour's goldens
    tests/run-in-docker.sh [IMAGE] [--update-goldens]

Refreshing the goldens is a deliberate act: do it when an algorithm change
is SUPPOSED to change the pages, look at the new PNGs before committing them,
and say in the commit message why they changed. The structural checks are
not refreshed by that flag -- if one of them fails, either the change is a
regression or the expectation below has to be edited by hand, on purpose.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
EXAMPLES = os.path.join(ROOT, "examples")
GOLDEN_ROOT = os.path.join(HERE, "golden")

# Golden renders are 50 dpi: 414x585 for an A4 page. Enough to see a binding
# column, a missing line or a turned page, and small enough to commit.
RENDER_DPI = 50

# TOLERANCE -- measured, not guessed (2026-09, 50 dpi renders):
#
#                                               MAE (0-255)  pixels moved >64
#   light image arm64 vs Homebrew IM 7.1.2 HDRI       0.00         0.00%
#   light golden vs the FULL image (Debian, non-HDRI):
#     sample-photo                                    0.01         0.00%
#     notebook-photo (the quad lands 2 px wider, so
#     every stroke shifts by a fraction of a pixel)   3.21         1.28%
#   notebook with RING_MIN = 999 (binding kept)      11.33         7.39%
#
# Within one image the render is stable across architectures and across an
# ImageMagick minor release, but switching the ImageMagick BUILD (HDRI or
# not) moves the demo page by MAE ~3 -- a sub-pixel shift, nothing visible.
# Hence one golden set PER IMAGE FLAVOUR (tests/golden/light, .../full), and
# thresholds loose enough that even a whole-build switch inside one flavour
# (3.21 / 1.28%) passes, while the broken binding rule is ~3x over both.
# The image metric is the second net: the structural checks catch a real
# break first and say what broke.
MAX_MAE = 4.0
MAX_CHANGED = 3.0  # percent of pixels whose worst channel moved by more than 64

# Exact A4 as a4norm writes it: 2480x3508 px at 300 dpi = 595.20 x 841.92 pt,
# which is ISO A4 (595.276 x 841.890 pt) to within 0.03 mm.
A4_PT = (595.20, 841.92)
ISO_A4_PT = (595.276, 841.890)


class Fail(Exception):
    pass


# ---------------------------------------------------------------- utilities

def sh(*cmd, **kw):
    return subprocess.run(cmd, check=True, capture_output=True, **kw)


def raw_rgb(path):
    """(w, h, bytes) of an image, 8-bit RGB, via ImageMagick."""
    w, h = (int(x) for x in sh("magick", "identify", "-format", "%w %h",
                               path + "[0]").stdout.split())
    data = sh("magick", path + "[0]", "-alpha", "off", "-colorspace", "sRGB",
              "-depth", "8", "rgb:-").stdout
    return w, h, data


def pdf_facts(pdf):
    info = sh("pdfinfo", "-box", pdf).stdout.decode()
    pages = int(re.search(r"^Pages:\s+(\d+)", info, re.M).group(1))
    box = [float(x) for x in
           re.search(r"^MediaBox:\s+(.+)$", info, re.M).group(1).split()]
    rot = int(re.search(r"^Page rot:\s+(\d+)", info, re.M).group(1))
    return pages, box, rot


def parse_report(text):
    """Pull the structural facts out of a4norm's per-page report."""
    lines = [l.strip()[2:] for l in text.splitlines()
             if l.strip().startswith("- ")]
    r = {"lines": lines, "kept": {}, "cut": None, "page": None,
         "rectified": None, "rotated": None, "fit": None, "ink_share": None,
         "document": None}
    for l in lines:
        m = re.match(r"rectified the sheet quad \((\d+)% of the frame\) -> (\d+)x(\d+)", l)
        if m:
            r["rectified"] = tuple(int(x) for x in m.groups())
        if l.startswith("kept as document, not erased or cut: "):
            for part in l.split(": ", 1)[1].split("; "):
                m = re.match(r"(\w+) \(structure ([\d.]+)%, brightness (\d+)% of paper\)", part)
                if m:
                    r["kept"][m.group(1)] = (float(m.group(2)), int(m.group(3)))
        m = re.match(r"cut a solid band of it away: L/R/T/B = (\d+)/(\d+)/(\d+)/(\d+) px", l)
        if m:
            r["cut"] = dict(zip(("left", "right", "top", "bottom"),
                                (int(x) for x in m.groups())))
        m = re.match(r"page: (\d+)x(\d+)px @ (\d+)dpi", l)
        if m:
            r["page"] = tuple(int(x) for x in m.groups())
        if l.startswith("rotated "):
            r["rotated"] = l
        m = re.match(r"fit: (.+?); scale ([\d.]+), offset ([+-]\d+)([+-]\d+)", l)
        if m:
            r["fit"] = (m.group(1), float(m.group(2)),
                        int(m.group(3)), int(m.group(4)))
        m = re.match(r"ink neutralized, coloured ink kept on ([\d.]+)% of the page", l)
        if m:
            r["ink_share"] = float(m.group(1))
        if "treated as a document" in l:
            r["document"] = True
        if l.startswith("no document in the frame"):
            r["document"] = False
    return r


class Page:
    """Pixel facts of a rendered page."""

    def __init__(self, path):
        self.w, self.h, self.data = raw_rgb(path)
        n = self.w * self.h
        self.lum = bytearray(n)
        self.chroma = bytearray(n)
        self.blue = 0
        d = self.data
        for i in range(n):
            r, g, b = d[3 * i], d[3 * i + 1], d[3 * i + 2]
            self.lum[i] = (299 * r + 587 * g + 114 * b) // 1000
            self.chroma[i] = max(r, g, b) - min(r, g, b)
            if b - max(r, g) >= 40 and self.lum[i] < 200:
                self.blue += 1
        self.n = n

    def share(self, pred):
        return 100.0 * sum(1 for i in range(self.n) if pred(i)) / self.n

    def ink_share(self, thr=160):
        return 100.0 * sum(1 for v in self.lum if v < thr) / self.n

    def column_ink(self, x0, x1, thr=160):
        """% of pixels darker than `thr` in columns [x0, x1)."""
        cnt = 0
        for y in range(self.h):
            row = y * self.w
            cnt += sum(1 for x in range(x0, x1) if self.lum[row + x] < thr)
        return 100.0 * cnt / ((x1 - x0) * self.h)

    def line_direction(self, thr=160):
        """How much more the ink varies down the page than across it.

        Lines of text that run across the page make the per-ROW ink profile
        swing hard between line and gap, while the per-COLUMN profile stays
        flat. A page whose picture got turned 90 degrees swaps the two. The
        ratio is the coefficient of variation of the row profile over that
        of the column profile, both inside the ink bounding box.
        """
        rows = [0] * self.h
        cols = [0] * self.w
        for y in range(self.h):
            row = y * self.w
            for x in range(self.w):
                if self.lum[row + x] < thr:
                    rows[y] += 1
                    cols[x] += 1

        def cv(v):
            nz = [i for i, c in enumerate(v) if c]
            if not nz:
                return 0.0
            v = v[nz[0]:nz[-1] + 1]
            m = sum(v) / len(v)
            return (sum((c - m) ** 2 for c in v) / len(v)) ** 0.5 / m if m else 0.0

        c = cv(cols)
        return cv(rows) / c if c else 99.0


def compare(a_path, b_path):
    aw, ah, a = raw_rgb(a_path)
    bw, bh, b = raw_rgb(b_path)
    if (aw, ah) != (bw, bh):
        raise Fail(f"render is {aw}x{ah}, golden is {bw}x{bh} -- the page "
                   f"size or orientation changed")
    tot, big = 0, 0
    for i in range(aw * ah):
        j = 3 * i
        d = max(abs(a[j] - b[j]), abs(a[j + 1] - b[j + 1]),
                abs(a[j + 2] - b[j + 2]))
        tot += abs(a[j] - b[j]) + abs(a[j + 1] - b[j + 1]) + abs(a[j + 2] - b[j + 2])
        if d > 64:
            big += 1
    return tot / (3.0 * aw * ah), 100.0 * big / (aw * ah)


# ---------------------------------------------------------------- the cases
#
# Each check is (description, predicate over (report, page)). The description
# says what the page must BE; the failure message adds what was measured.

def check_a4_portrait(rep, page):
    if rep["page"] != (2480, 3508, 300):
        raise Fail(f"page raster is {rep['page']}, expected a portrait A4 "
                   f"2480x3508 @ 300 dpi")


def notebook_checks():
    def rectified(rep, page):
        if not rep["rectified"]:
            raise Fail("the sheet quad was not rectified -- perspective stays "
                       "a trapezoid. Report:\n    " + "\n    ".join(rep["lines"]))
        pct, w, h = rep["rectified"]
        if not (20 <= pct <= 40):
            raise Fail(f"quad covers {pct}% of the frame, expected 20-40% "
                       f"(the notebook page, not the desk or the laptop)")
        if not (0.9 <= w / h <= 1.15):
            raise Fail(f"rectified sheet is {w}x{h}; the notebook page is "
                       f"nearly square, expected aspect 0.9-1.15")

    def binding_cut(rep, page):
        if "left" in rep["kept"]:
            st, dk = rep["kept"]["left"]
            raise Fail(f"LEFT side was judged DOCUMENT (structure {st}%, "
                       f"brightness {dk}% of paper) -- the spiral binding "
                       f"was not recognised by _is_binding and stays on the "
                       f"page (RING_MIN / RING_COVER / RING_SPREAD?)")
        if not rep["cut"] or rep["cut"]["left"] <= 0:
            raise Fail(f"no band was cut from the left side (cut = "
                       f"{rep['cut']}) -- the binding is still there")

    def other_sides_kept(rep, page):
        missing = [s for s in ("right", "top", "bottom") if s not in rep["kept"]]
        if missing:
            raise Fail(f"sides {missing} were NOT judged document -- the "
                       f"handwriting there is at risk of being erased or cut")
        if rep["cut"] and any(rep["cut"][s] for s in ("right", "top", "bottom")):
            raise Fail(f"cut away more than the binding: {rep['cut']}")

    def page_not_picture(rep, page):
        if rep["rotated"]:
            raise Fail(f"the picture was rotated ({rep['rotated']}); with "
                       f"--rotate auto only the page may turn")
        check_a4_portrait(rep, page)
        mode, scale, ox, oy = rep["fit"]
        if not mode.startswith("frame (the rectified quad is the sheet)"):
            raise Fail(f"fit mode is {mode!r}, expected the rectified frame")
        # a near-square sheet on a portrait page: full width, centred vertically
        if ox != 0 or not (300 <= oy <= 700):
            raise Fail(f"sheet placed at offset {ox:+d}{oy:+d}; expected it "
                       f"to fill the page width and sit centred vertically "
                       f"(about +0+480)")

    def lines_across(rep, page):
        ratio = page.line_direction()
        if ratio < 1.5:
            raise Fail(f"row/column ink-variation ratio is {ratio:.2f} "
                       f"(expected >= 1.5; measured 2.2, and 0.45 with the "
                       f"picture turned 90 degrees): the ink no longer reads "
                       f"as lines across the page -- the picture was turned, "
                       f"or a dark column dominates it")

    def not_blank(rep, page):
        s = page.ink_share()
        if not (1.0 <= s <= 12.0):
            raise Fail(f"{s:.2f}% of the page is ink, expected 1-12% "
                       f"(blank, or buried in dark junk)")

    def no_binding_in_margin(rep, page):
        # the sheet fills the page width, so the binding would sit in the
        # leftmost few percent of the page
        s = page.column_ink(0, max(1, page.w * 4 // 100))
        if s > 2.0:
            raise Fail(f"the left 4% of the page is {s:.1f}% ink (expected "
                       f"<= 2%): something dark is standing in the margin "
                       f"where the spiral binding was")

    def pencil_grey(rep, page):
        ink = [i for i in range(page.n) if page.lum[i] < 200]
        mid = sum(1 for i in ink if page.lum[i] >= 60)
        share = 100.0 * mid / len(ink) if ink else 0
        if share < 50:
            raise Fail(f"only {share:.0f}% of the ink is mid-grey (expected "
                       f">= 50%): the pencil was pushed to black")
        tinted = page.share(lambda i: page.chroma[i] > 40)
        if tinted > 0.3:
            raise Fail(f"{tinted:.2f}% of the page is strongly coloured "
                       f"(expected <= 0.3%): the pencil picked up a tint")

    return [
        ("perspective is rectified onto the notebook page", rectified),
        ("spiral binding on the LEFT is judged a binding and cut", binding_cut),
        ("right/top/bottom are kept as document, nothing else cut", other_sides_kept),
        ("the page turns, not the picture (portrait A4, sheet unrotated)", page_not_picture),
        ("handwriting runs across the page", lines_across),
        ("the page is not blank", not_blank),
        ("no binding marks left in the left margin", no_binding_in_margin),
        ("pencil stays grey and untinted", pencil_grey),
    ]


def sample_checks():
    def document(rep, page):
        if rep["document"] is not True:
            raise Fail("the letter was not treated as a document. Report:\n    "
                       + "\n    ".join(rep["lines"]))
        check_a4_portrait(rep, page)
        if not any(l.startswith("deskewed ") for l in rep["lines"]):
            raise Fail("the tilted letter was not deskewed")
        mode = rep["fit"][0]
        if not mode.startswith("edges "):
            raise Fail(f"fit mode is {mode!r}, expected the sheet-edge fit")
        if rep["rotated"]:
            raise Fail(f"the picture was rotated ({rep['rotated']})")

    def lines_across(rep, page):
        ratio = page.line_direction()
        if ratio < 1.5:
            raise Fail(f"row/column ink-variation ratio is {ratio:.2f} "
                       f"(expected >= 1.5): the text no longer runs across")

    def not_blank(rep, page):
        s = page.ink_share()
        if not (0.3 <= s <= 8.0):
            raise Fail(f"{s:.2f}% of the page is ink, expected 0.3-8%")

    def blue_signature(rep, page):
        share = 100.0 * page.blue / page.n
        # 0.023% at 50 dpi (the stroke is thin); exactly 0 with --gray
        if share < 0.01:
            raise Fail(f"only {share:.4f}% of the page is blue ink (expected "
                       f">= 0.01%, measured 0.023%): the blue signature was "
                       f"neutralised")
        if rep["ink_share"] is None or not (0.2 <= rep["ink_share"] <= 1.5):
            raise Fail(f"a4norm kept coloured ink on {rep['ink_share']}% of "
                       f"the page, expected 0.2-1.5% (the signature)")

    return [
        ("treated as a document, deskewed, fitted by its sheet edges", document),
        ("text runs across the page", lines_across),
        ("the page is not blank", not_blank),
        ("the blue signature stays blue", blue_signature),
    ]


CASES = [
    ("notebook-photo", "notebook-photo.jpg", notebook_checks),
    ("sample-photo", "sample-photo.jpg", sample_checks),
]


# ---------------------------------------------------------------- runner

def run_case(name, src, checks, a4norm, update, artifacts, golden_dir):
    failures = []
    with tempfile.TemporaryDirectory(prefix=f"a4reg-{name}-") as wd:
        pdf = os.path.join(wd, name + ".pdf")
        p = subprocess.run([a4norm, "-o", pdf, os.path.join(EXAMPLES, src)],
                           capture_output=True, text=True)
        if p.returncode != 0:
            return ([f"a4norm exited {p.returncode}:\n{p.stderr}"], "no output",
                    p.stdout)
        report = parse_report(p.stdout)

        pages, box, rot = pdf_facts(pdf)
        if pages != 1:
            failures.append(f"PDF has {pages} pages, expected 1")
        if [round(x, 2) for x in box] != [0.0, 0.0, *A4_PT] or rot != 0:
            failures.append(f"MediaBox is {box} (rotation {rot}), expected "
                            f"exactly [0 0 {A4_PT[0]} {A4_PT[1]}], unrotated")
        short, long_ = sorted(box[2:4])
        if abs(short - ISO_A4_PT[0]) > 0.5 or abs(long_ - ISO_A4_PT[1]) > 0.5:
            failures.append(f"MediaBox {box} is not ISO A4")

        png_base = os.path.join(wd, "render")
        sh("pdftoppm", "-r", str(RENDER_DPI), "-png", "-singlefile", pdf, png_base)
        render = png_base + ".png"
        page = Page(render)

        for desc, fn in checks():
            try:
                fn(report, page)
            except Fail as e:
                failures.append(f"{desc}\n      {e}")
            except Exception as e:  # a parse miss is a failure, not a crash
                failures.append(f"{desc}\n      could not evaluate: "
                                f"{type(e).__name__}: {e}")

        golden = os.path.join(golden_dir, name + ".png")
        if update:
            os.makedirs(golden_dir, exist_ok=True)
            sh("magick", render, "-strip", "-define",
               "png:compression-level=9", golden)
            metric = "golden rewritten"
        elif not os.path.exists(golden):
            failures.append(f"no golden at {os.path.relpath(golden, ROOT)} "
                            f"-- run with --update-goldens")
            metric = "no golden"
        else:
            try:
                mae, changed = compare(render, golden)
                metric = f"MAE {mae:.2f} (max {MAX_MAE}), changed {changed:.2f}% (max {MAX_CHANGED}%)"
                if mae > MAX_MAE or changed > MAX_CHANGED:
                    failures.append(
                        f"rendered page drifted from the golden: {metric}\n"
                        f"      if the change is intended, look at the new "
                        f"render and refresh with --update-goldens")
            except Fail as e:
                failures.append(f"golden comparison: {e}")
                metric = "size mismatch"

        if artifacts:
            os.makedirs(artifacts, exist_ok=True)
            shutil.copy(pdf, os.path.join(artifacts, name + ".pdf"))
            shutil.copy(render, os.path.join(artifacts, name + "-render.png"))
            with open(os.path.join(artifacts, name + "-report.txt"), "w") as f:
                f.write(p.stdout)
    return failures, metric, p.stdout


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--a4norm", default=shutil.which("a4norm") or
                    os.path.join(ROOT, "a4norm"),
                    help="the a4norm to test (default: the one on PATH)")
    ap.add_argument("--update-goldens", action="store_true",
                    help="rewrite tests/golden/<flavor>/*.png from this run")
    ap.add_argument("--artifacts", help="copy PDFs, renders and reports here")
    ap.add_argument("--only", help="run one case by name")
    ap.add_argument("--flavor", choices=("light", "full"),
                    help="which golden set to compare with (default: 'full' "
                         "when a4norm-seg is on PATH, as in the :full image)")
    a = ap.parse_args()
    flavor = a.flavor or ("full" if shutil.which("a4norm-seg") else "light")
    golden_dir = os.path.join(GOLDEN_ROOT, flavor)

    print(f"a4norm under test: {a.a4norm}  (goldens: {flavor})")
    print(sh("magick", "-version").stdout.decode().splitlines()[0])
    bad = 0
    for name, src, checks in CASES:
        if a.only and a.only != name:
            continue
        failures, metric, out = run_case(name, src, checks, a.a4norm,
                                         a.update_goldens, a.artifacts,
                                         golden_dir)
        if failures:
            bad += 1
            print(f"\nFAIL  {name}  [{metric}]")
            for f in failures:
                print(f"  x {f}")
            print("  a4norm report:")
            for l in out.splitlines():
                print(f"    {l}")
        else:
            print(f"ok    {name}  [{metric}]")
    if bad:
        print(f"\n{bad} example(s) regressed. A change to a4norm broke a "
              f"public example -- the landing-page demo is notebook-photo.")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
