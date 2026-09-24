#!/usr/bin/env python3
"""Run a4norm over the LOCAL photo corpus in tests/corpus/.

The public examples in examples/ are what CI can see. Real documents -- a
passport, a record book, a bank card -- carry personal data and must never be
committed, so they live in tests/corpus/, which is in .gitignore and
.dockerignore, and exist only on the machines that run this. The runner and
the format are public; the photos and their expectations are not.

    tests/corpus/
        cases.json            what each photo must come out as (see below)
        <photo>.jpg ...       the photos, named for what makes them hard
        out/                  written by this script: PDFs, renders, reports,
                              and sheet.png -- every input next to its page

cases.json is a list of objects:

    {"file": "ru-internal-sideways-thumb-dark-desk.jpg",
     "note": "the photo the spread support was written for",
     "args": [],                       # extra a4norm flags, optional
     "expect": {"spread": true,        # a spread was found and joined
                "face_photo": true,    # a face photo was kept out of the
                                       # paper treatment, and on the page it
                                       # sits in the lower-left quarter (as
                                       # a4norm reports it)
                "lines_across": true,  # text runs across the page
                "turn": 90,            # the spread was turned this much
                "spread_aspect": [1.3, 1.55],   # long/short of the joined
                                       # spread: 176x125 mm is 1.41
                "clean_corners": true},  # no grey left in the page corners
     "known_fail": "why"}              # optional: reported, not counted

A case can also be several photos going into one document -- a card's front
and back shot one after the other:

    {"name": "de-ausweis-pair", "files": ["front.jpg", "back.jpg"],
     "expect": {"cards": 2,            # cards found, over all the photos
                "fronts": 1,           # of them with a face photo
                "pages": 1}}           # pages in the PDF

A case marked known_fail is a photo the tool does not handle YET: it stays in
the corpus as the next thing to fix, is reported as xfail while it fails, and
as XPASS (so the mark can be removed) once it passes.

    python3 tests/corpus.py                 # all cases
    python3 tests/corpus.py --only NAME     # one case (file name, no suffix ok)
    python3 tests/corpus.py --a4norm PATH   # a different a4norm

Stdlib only, like tests/regression.py, whose report parser and page facts
this reuses.
"""
import argparse
import importlib.machinery
import importlib.util
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
CORPUS = os.path.join(HERE, "corpus")
OUT = os.path.join(CORPUS, "out")

sys.path.insert(0, HERE)
import regression as R  # noqa: E402  (parse_report, Page, A4 constants)


def load_a4norm(path):
    """The a4norm under test as a module, for its photo-block detector."""
    loader = importlib.machinery.SourceFileLoader("a4norm_mod", path)
    spec = importlib.util.spec_from_loader("a4norm_mod", loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


def check(case, rep, page, render, A, pdf=None):
    """Failures of one case, as a list of strings."""
    exp = case.get("expect", {})
    bad = []
    lines = rep["lines"]
    spread = any(l.startswith("spread: two facing pages") for l in lines)
    if "spread" in exp and spread != exp["spread"]:
        bad.append(f"spread found: {spread}, expected {exp['spread']}")
    if "spread_aspect" in exp:
        # A passport page is 125 x 88 mm, so a joined spread is 176 x 125:
        # long/short 1.41 whichever way it lies. A page edge fitted inside the
        # page -- glare, a patterned desk that passed for paper -- cuts a
        # strip of the document away without failing any other check.
        lo, hi = exp["spread_aspect"]
        for l in lines:
            m = re.search(r"joined at the fold -> (\d+)x(\d+)", l)
            if m:
                a, b = int(m.group(1)), int(m.group(2))
                r = max(a, b) / min(a, b)
                if not lo <= r <= hi:
                    bad.append(f"spread is {a}x{b}, long/short {r:.2f}; a "
                               f"passport spread is {lo}-{hi} -- a strip of "
                               f"it was cut off, or something else was added")
    card_lines = [l for l in lines if re.match(r"card \d+: rectified", l)]
    if "cards" in exp and len(card_lines) != exp["cards"]:
        bad.append(f"{len(card_lines)} card(s) found, expected {exp['cards']}")
    if "fronts" in exp:
        fronts = sum(1 for l in card_lines if l.endswith("— front"))
        if fronts != exp["fronts"]:
            bad.append(f"{fronts} front(s) with a face photo, expected "
                       f"{exp['fronts']}")
    if exp.get("front_on_top") and card_lines:
        # the rendered page's upper card must hold the face: its left
        # third darker than the lower card's
        top = page.share(lambda i: i // page.w < page.h // 2
                         and i % page.w < page.w // 2 and page.lum[i] < 120)
        bottom = page.share(lambda i: i // page.w >= page.h // 2
                            and i % page.w < page.w // 2 and page.lum[i] < 120)
        if top <= bottom:
            bad.append(f"the front is not on top (dark on the left: top "
                       f"{top:.2f}%, bottom {bottom:.2f}%)")
    if exp.get("clean_corners"):
        # A white page does not end in grey corners. What a shadow over the
        # sheet's corner leaves is GRAIN: many small separate grey flecks.
        # A share of grey pixels cannot tell that from print that reaches a
        # corner (a footer rule ending there measured 0.5%), a count of
        # flecks can: small 8-connected grey marks in each corner box.
        # at 150 dpi: at the 50 dpi of the other checks the flecks are
        # averaged away, and a page full of grain passes as clean
        hi = page
        if pdf:
            base = os.path.splitext(pdf)[0] + "-150"
            R.sh("pdftoppm", "-r", "150", "-png", "-singlefile", pdf, base)
            hi = R.Page(base + ".png")
            os.unlink(base + ".png")
        page = hi
        cw, ch = page.w // 10, page.h // 10
        for name, x0, y0 in (("top-left", 0, 0), ("top-right", page.w - cw, 0),
                             ("bottom-left", 0, page.h - ch),
                             ("bottom-right", page.w - cw, page.h - ch)):
            grey = {(x, y) for y in range(y0, y0 + ch) for x in range(x0, x0 + cw)
                    if page.lum[y * page.w + x] < 230}
            flecks = 0
            while grey:
                stack = [grey.pop()]
                size = 1
                while stack:
                    x, y = stack.pop()
                    for dx in (-1, 0, 1):
                        for dy in (-1, 0, 1):
                            q = (x + dx, y + dy)
                            if q in grey:
                                grey.discard(q); stack.append(q); size += 1
                if size <= 150:     # 4 mm² at 150 dpi, or less
                    flecks += 1
            if flecks > 2:
                bad.append(f"{name} corner holds {flecks} grey flecks "
                           f"(expected <= 2): shadow grain left on the paper")
    if "turn" in exp:
        got = 0
        for l in lines:
            if l.startswith("rotated "):
                got = int(l.split()[1].rstrip("°"))
        if got != exp["turn"]:
            bad.append(f"turned {got}°, expected {exp['turn']}°")
    copy = any(l.startswith("colour copy:") for l in lines)
    # On a colour copy the two measures below are read on a page that keeps
    # its guilloche, and they misread it both ways (a correctly turned spread
    # measured 0.53 "across", a present photo 0.10 "dark"). There the "turn"
    # expectation already fixes the orientation, and the contact sheet is
    # the check that the photo and the ornament are there.
    if exp.get("lines_across") and not copy:
        # a4norm's own run-length measure, not regression.py's profile
        # variance: on a passport page the photo and the vertical serial
        # number swing the column profile as hard as the text swings the rows
        # (1.22 on a correctly turned spread)
        # Dark print only: a colour copy keeps the guilloche, whose light
        # lines run every way and at the default bar read as text.
        across, along = A._ink_runs(render, thr=55)
        ratio = across / max(1, along)
        if ratio < 1.6:
            bad.append(f"text does not run across the page (ink in runs "
                       f"across/along {ratio:.2f}, expected >= 1.6)")
    if "face_photo" in exp and copy:
        pass
    elif "face_photo" in exp:
        box = None
        for l in lines:
            m = re.match(r"face photo at (\d+)x(\d+)\+(\d+)\+(\d+) of (\d+)x(\d+)", l)
            if m:
                box = [int(g) for g in m.groups()]
        if (box is not None) != exp["face_photo"]:
            bad.append(f"face photo kept apart: {box is not None}, expected "
                       f"{exp['face_photo']}")
        if box and exp["face_photo"]:
            # the box is in the page's own coordinates, upright, before the
            # fit -- where it sits there is where it sits on the A4
            bw, bh, bx, by, pw, ph = box
            cx, cy = (bx + bw / 2) / pw, (by + bh / 2) / ph
            if not (cx < 0.5 and cy > 0.5):
                bad.append(f"face photo centred at {cx:.0%} across, {cy:.0%} "
                           f"down; expected the lower-left quarter (the page "
                           f"is turned wrong)")
    if rep["page"] and rep["page"][2] in R.A4_PX:
        if rep["page"][:2] != R.A4_PX[rep["page"][2]]:
            bad.append(f"page raster {rep['page']} is not a portrait A4")
    return bad


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--a4norm", default=os.path.join(ROOT, "a4norm"))
    ap.add_argument("--only")
    a = ap.parse_args()

    cases_path = os.path.join(CORPUS, "cases.json")
    if not os.path.exists(cases_path):
        print(f"no corpus here ({cases_path} is missing) -- nothing to run")
        return 0
    cases = json.load(open(cases_path))
    os.makedirs(OUT, exist_ok=True)
    A = load_a4norm(a.a4norm)

    counts = {"ok": 0, "FAIL": 0, "xfail": 0, "XPASS": 0}
    rows = []
    for case in cases:
        files = case.get("files") or [case["file"]]
        name = case.get("name") or os.path.splitext(files[0])[0]
        if a.only and a.only not in (name, files[0]):
            continue
        srcs = [os.path.join(CORPUS, f) for f in files]
        src = srcs[0]
        pdf = os.path.join(OUT, name + ".pdf")
        p = subprocess.run([a.a4norm, *case.get("args", []), "-o", pdf, *srcs],
                           capture_output=True, text=True)
        open(os.path.join(OUT, name + ".txt"), "w").write(p.stdout + p.stderr)
        if p.returncode != 0:
            bad = [f"a4norm exited {p.returncode}: {p.stderr.strip()}"]
            render = None
        else:
            rep = R.parse_report(p.stdout)
            base = os.path.join(OUT, name)
            R.sh("pdftoppm", "-r", str(R.RENDER_DPI), "-png", "-singlefile",
                 pdf, base)
            render = base + ".png"
            bad = check(case, rep, R.Page(render), render, A, pdf)
            npages = R.pdf_facts(pdf)[0]
            if npages != case.get("expect", {}).get("pages", npages):
                bad.append(f"{npages} page(s) in the PDF, expected "
                           f"{case['expect']['pages']}")
        known = case.get("known_fail")
        if bad and known:
            status = "xfail"
        elif bad:
            status = "FAIL"
        elif known:
            status = "XPASS"
        else:
            status = "ok"
        counts[status] += 1
        size = os.path.getsize(pdf) // 1024 if os.path.exists(pdf) else 0
        print(f"{status:5s} {name}  ({size} KB)")
        if known and bad:
            print(f"      known: {known}")
        for b in bad:
            print(f"  x {b}")
        if status == "XPASS":
            print("      passes now -- drop its known_fail mark")
        rows.append((src, render))

    # every input beside the page it became, for the eye -- the checks above
    # are necessary, never sufficient
    if rows:
        tiles = []
        for i, (src, render) in enumerate(rows):
            t = os.path.join(OUT, f".tile-{i}.png")
            args = ["magick", "(", src + "[0]", "-auto-orient", "-resize",
                    "400x400", "-background", "white", "-gravity", "center",
                    "-extent", "420x420", ")"]
            if render:
                args += ["(", render, "-resize", "300x420", "-background",
                         "white", "-gravity", "center", "-extent", "320x420", ")"]
            else:
                args += ["-size", "320x420", "xc:white"]
            # no caption: -annotate needs a font, and a bare ImageMagick has
            # none configured. The rows are in the order printed above.
            args += ["+append", "-bordercolor", "gray70", "-border", "1", t]
            subprocess.run(args, check=True)
            tiles.append(t)
        subprocess.run(["magick", *tiles, "-append",
                        os.path.join(OUT, "sheet.png")], check=True)
        for t in tiles:
            os.unlink(t)
        print(f"\ncontact sheet: {os.path.join(OUT, 'sheet.png')}")
    print("  ".join(f"{k} {v}" for k, v in counts.items() if v))
    return 1 if counts["FAIL"] else 0


if __name__ == "__main__":
    sys.exit(main())
