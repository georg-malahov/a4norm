#!/usr/bin/env python3
"""Byte-for-byte snapshot of what a4norm produces, for refactoring.

    python3 tests/snapshot.py save       # record tests/out/snapshot.json
    python3 tests/snapshot.py check      # rerun, compare, list what changed
    python3 tests/snapshot.py compare OTHER [FILTER]
                                         # the same runs through the script and
                                         # through OTHER (the Rust binary):
                                         # report lines and page PSNR side by side

A refactor must not change a single output byte. This runs the a4norm in
this checkout over every public example under a spread of flags (so the
paths a default run never takes -- --gray, --format jpg, --photo on, a forced
fit -- are covered too), and over the local corpus if there is one, and
records the SHA-256 of every file written plus the report a4norm printed,
with the temporary paths taken out. `check` does the same and names every
case whose bytes or report differ.

It is not a quality test -- tests/run.sh is. It answers one question only:
did anything at all change? Runs in parallel; stdlib only.
"""
import concurrent.futures
import glob
import hashlib
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
A4NORM = os.environ.get("A4NORM") or os.path.join(ROOT, "a4norm")
SNAP = os.path.join(HERE, "out", "snapshot.json")

FLAGS = [
    [], ["--gray"], ["--format", "jpg"], ["--dpi", "300"], ["--photo", "on"],
    ["--rectify", "off"], ["--edges", "off"], ["--cards", "off"],
    ["--fit", "content"], ["--no-despeckle"], ["--no-haze"],
    ["--spread-scan"], ["--landscape"], ["--rotate", "90"],
]


def jobs():
    out = []
    for ex in sorted(glob.glob(os.path.join(ROOT, "examples", "*"))):
        for fl in FLAGS:
            out.append((f"{os.path.basename(ex)} {' '.join(fl)}".strip(),
                        fl, [ex]))
    cases = os.path.join(HERE, "corpus", "cases.json")
    if os.path.exists(cases):
        for c in json.load(open(cases)):
            files = c.get("files") or [c["file"]]
            name = c.get("name") or files[0]
            srcs = [os.path.join(HERE, "corpus", f) for f in files]
            out.append((f"corpus/{name}", c.get("args", []), srcs))
            out.append((f"corpus/{name} --spread-scan",
                        c.get("args", []) + ["--spread-scan"], srcs))
    return out


def run(job, a4norm=None):
    name, flags, srcs = job
    with tempfile.TemporaryDirectory(prefix="a4snap-") as wd:
        ext = ".jpg" if "jpg" in flags else ".pdf"
        dst = os.path.join(wd, "out" + ext)
        p = subprocess.run([a4norm or A4NORM, *flags, "-o", dst, *srcs],
                           capture_output=True, text=True)
        files = {}
        for f in sorted(os.listdir(wd)):
            files[f] = hashlib.sha256(
                open(os.path.join(wd, f), "rb").read()).hexdigest()
        report = (p.stdout + p.stderr).replace(wd, "<out>")
        report = "\n".join(l for l in report.splitlines()
                           if not l.lstrip().startswith("-> "))
    return name, {"exit": p.returncode, "files": files, "report": report}


def pages(path, wd, tag):
    """The page images of an output: the JPEGs inside a PDF, or the JPEGs."""
    if path.endswith(".pdf"):
        subprocess.run(["pdfimages", "-j", path, os.path.join(wd, tag)],
                       capture_output=True)
        return sorted(os.path.join(wd, f) for f in os.listdir(wd)
                      if f.startswith(tag + "-"))
    stem = path[:-4]
    return sorted(glob.glob(stem + "*.jpg"))


def psnr(a, b):
    p = subprocess.run(["magick", "compare", "-metric", "PSNR", a, b, "null:"],
                       capture_output=True, text=True)
    out = (p.stderr or p.stdout).split()
    if not out:
        return "?"
    try:
        return f"{float(out[0]):.1f}"
    except ValueError:
        return "size" if "size" in p.stderr.lower() or "differ" in p.stderr.lower() else out[0]


def compare_one(job, other):
    """One case through both: (name, same report?, first difference, PSNRs,
    seconds each)."""
    import time
    name, flags, srcs = job
    res = []
    with tempfile.TemporaryDirectory(prefix="a4cmp-") as wd:
        ext = ".jpg" if "jpg" in flags else ".pdf"
        outs = []
        for tag, exe in (("py", A4NORM), ("rs", other)):
            dst = os.path.join(wd, tag + "-out" + ext)
            t = time.time()
            p = subprocess.run([exe, *flags, "-o", dst, *srcs],
                               capture_output=True, text=True)
            secs = time.time() - t
            rep = (p.stdout + p.stderr).replace(wd, "<out>")
            rep = [l.replace(tag + "-out", "out") for l in rep.splitlines()
                   if not l.lstrip().startswith("-> ")]
            outs.append((dst, rep, p.returncode, secs))
        (pd, pr, pe, ps), (rd, rr, re_, rsec) = outs
        diff = ""
        for x, y in zip(pr + [""] * len(rr), rr + [""] * len(pr)):
            if x != y:
                diff = f"py: {x.strip()}\n        rs: {y.strip()}"
                break
        pp, rp = pages(pd, wd, "pp"), pages(rd, wd, "rp")
        ps_ = [psnr(a, b) for a, b in zip(pp, rp)]
        if len(pp) != len(rp):
            ps_.append(f"pages {len(pp)}/{len(rp)}")
        same = pr == rr and pe == re_
    return name, same, diff, ps_, ps, rsec


def compare(other, filt):
    js = [j for j in jobs() if not filt or (filt[1:] not in j[0] if filt.startswith("!") else filt in j[0])]
    with concurrent.futures.ThreadPoolExecutor(max(1, (os.cpu_count() or 4) // 2)) as ex:
        got = list(ex.map(lambda j: compare_one(j, other), js))
    same = 0
    tpy = trs = 0.0
    for name, ok, diff, ps, a, b in got:
        same += ok
        tpy += a
        trs += b
        print(f"{'SAME' if ok else 'DIFF'}  {name:55s} psnr {' '.join(ps) or '-':12s} "
              f"py {a:5.1f}s rs {b:5.2f}s")
        if diff:
            print(f"        {diff}")
    print(f"{same}/{len(got)} reports identical; time py {tpy:.0f}s rs {trs:.1f}s")
    return 0


def main():
    if len(sys.argv) >= 3 and sys.argv[1] == "compare":
        return compare(os.path.abspath(sys.argv[2]), sys.argv[3] if len(sys.argv) > 3 else "")
    if len(sys.argv) != 2 or sys.argv[1] not in ("save", "check"):
        sys.exit(__doc__.split("\n\n")[1])
    js = jobs()
    with concurrent.futures.ThreadPoolExecutor(os.cpu_count() or 4) as ex:
        now = dict(ex.map(run, js))
    if sys.argv[1] == "save":
        os.makedirs(os.path.dirname(SNAP), exist_ok=True)
        json.dump(now, open(SNAP, "w"), indent=1, sort_keys=True)
        print(f"saved {len(now)} runs to {SNAP}")
        return 0
    old = json.load(open(SNAP))
    bad = 0
    for name in sorted(set(old) | set(now)):
        a, b = old.get(name), now.get(name)
        if a == b:
            continue
        bad += 1
        print(f"CHANGED  {name}")
        if not a or not b:
            print("   only in", "the snapshot" if a else "this run")
            continue
        if a["files"] != b["files"]:
            print(f"   files: {a['files']} -> {b['files']}")
        if a["report"] != b["report"]:
            al, bl = a["report"].splitlines(), b["report"].splitlines()
            for x, y in zip(al + [""] * len(bl), bl + [""] * len(al)):
                if x != y:
                    print(f"   - {x}\n   + {y}")
                    break
        if a["exit"] != b["exit"]:
            print(f"   exit {a['exit']} -> {b['exit']}")
    print(f"{len(now) - bad} identical, {bad} changed")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
