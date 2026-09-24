#!/usr/bin/env python3
"""Turn the last test run into one page to look at: tests/out/report.html.

    python3 tests/report.py            # after tests/regression.py and/or
                                       # tests/corpus.py have run
    tests/run.sh --open                # the usual way: run, build, open

Reads what the runners left in tests/out/ (git-ignored):

    tests/out/examples/results.json    the public examples (tests/regression.py)
    tests/out/corpus/results.json      your local corpus (tests/corpus.py)

and writes, next to them, one self-contained HTML page: a row per case with
the input photo, the page a4norm made of it, the golden it is compared with
(public examples), the verdict and why, and a4norm's own report. Pictures are
small JPEG thumbnails in tests/out/thumbs/, linked to the full render and PDF,
so the page is light enough to open with a few hundred cases. Nothing leaves
the machine.

Stdlib only; thumbnails through ImageMagick, like everything else here.
"""
import datetime
import hashlib
import html
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "out")
THUMBS = os.path.join(OUT, "thumbs")
PAGE = os.path.join(OUT, "report.html")

STATUS = {
    "ok":    ("ok",       "pass",  "passes every check"),
    "FAIL":  ("FAIL",     "fail",  "a check failed"),
    "xfail": ("known",    "known", "a known failure, not counted"),
    "XPASS": ("XPASS",    "xpass", "was a known failure, passes now"),
}


def thumb(path, size=360):
    """A JPEG thumbnail of `path` (any format ImageMagick reads), relative
    to the page, or None."""
    if not path or not os.path.exists(path):
        return None
    st = os.stat(path)
    key = hashlib.sha1(f"{path}|{st.st_mtime_ns}|{size}".encode()).hexdigest()[:16]
    dst = os.path.join(THUMBS, key + ".jpg")
    if not os.path.exists(dst):
        os.makedirs(THUMBS, exist_ok=True)
        r = subprocess.run(["magick", path + "[0]", "-auto-orient",
                            "-resize", f"{size}x{size}>", "-strip",
                            "-quality", "82", dst], capture_output=True)
        if r.returncode != 0:
            return None
    return os.path.relpath(dst, OUT)


def rel(path):
    return os.path.relpath(path, OUT) if path and os.path.exists(path) else None


def figure(label, path, link=None):
    t = thumb(path)
    if not t:
        return f'<figure class="empty"><div>no {html.escape(label)}</div>' \
               f'<figcaption>{html.escape(label)}</figcaption></figure>'
    target = rel(link or path) or t
    return (f'<figure><a href="{html.escape(target)}" target="_blank">'
            f'<img loading="lazy" src="{html.escape(t)}" alt="{html.escape(label)}"></a>'
            f'<figcaption>{html.escape(label)}</figcaption></figure>')


def row(c):
    word, cls, title = STATUS.get(c["status"], (c["status"], "fail", ""))
    inputs = c.get("inputs") or [c["input"]]
    figs = "".join(figure("input" if len(inputs) == 1 else f"input {i + 1}", p)
                   for i, p in enumerate(inputs))
    figs += figure("result", c.get("render"), c.get("pdf"))
    if c.get("golden"):
        figs += figure("golden", c["golden"])
    why = ""
    if c.get("failures"):
        why = "<ul class=why>" + "".join(
            f"<li>{html.escape(f)}</li>" for f in c["failures"]) + "</ul>"
    known = (f'<p class=knownwhy><b>known:</b> {html.escape(c["known"])}</p>'
             if c.get("known") else "")
    note = f'<p class=note>{html.escape(c["note"])}</p>' if c.get("note") else ""
    metric = (f'<p class=metric>{html.escape(c["metric"])}</p>'
              if c.get("metric") else "")
    report = html.escape(c.get("report") or "").strip()
    pdf = rel(c.get("pdf"))
    pdflink = f' · <a href="{html.escape(pdf)}" target="_blank">PDF</a>' if pdf else ""
    return f'''
<article class="case {cls}" data-status="{cls}">
  <header>
    <span class="badge {cls}" title="{title}">{word}</span>
    <h3>{html.escape(c["name"])}</h3>
    <span class=links>{html.escape(os.path.basename(c["input"]))}{pdflink}</span>
  </header>
  {note}
  <div class=figs>{figs}</div>
  {metric}{why}{known}
  <details><summary>a4norm report</summary><pre>{report}</pre></details>
</article>'''


def section(title, blurb, data):
    if not data:
        return ""
    cases = data["cases"]
    counts = {}
    for c in cases:
        counts[c["status"]] = counts.get(c["status"], 0) + 1
    summary = " · ".join(f'<span class="badge {STATUS.get(k, (k, "fail"))[1]}">'
                         f'{STATUS.get(k, (k,))[0]} {v}</span>'
                         for k, v in sorted(counts.items()))
    # failures first, then the rest in run order
    order = {"FAIL": 0, "XPASS": 1, "xfail": 2, "ok": 3}
    cases = sorted(cases, key=lambda c: order.get(c["status"], 0))
    return (f'<section><h2>{title}</h2><p class=blurb>{blurb}</p>'
            f'<p class=summary>{summary}</p>'
            + "".join(row(c) for c in cases) + "</section>")


CSS = """
:root { --bg:#fbfaf8; --fg:#1d1d1b; --muted:#6b6a66; --card:#fff; --line:#e6e3dd;
  --ok:#1f7a4d; --ok-bg:#e3f3ea; --fail:#b3261e; --fail-bg:#fbe4e1;
  --known:#7a6a1f; --known-bg:#f5efd6; --xpass:#1f4f7a; --xpass-bg:#e1ecf7; }
@media (prefers-color-scheme: dark) { :root { --bg:#171716; --fg:#ecebe7;
  --muted:#a09e98; --card:#21211f; --line:#34332f; --ok:#6fcf9b; --ok-bg:#1c3226;
  --fail:#f28b82; --fail-bg:#3d1f1c; --known:#e2cf7a; --known-bg:#35301a;
  --xpass:#8ab4f8; --xpass-bg:#1c2a3a; } }
* { box-sizing: border-box; }
body { margin:0; background:var(--bg); color:var(--fg);
  font:15px/1.5 -apple-system, system-ui, "Segoe UI", sans-serif; }
main { max-width:1180px; margin:0 auto; padding:24px 16px 64px; }
h1 { font-size:22px; margin:0 0 4px; } h2 { font-size:18px; margin:32px 0 4px; }
h3 { font-size:15px; margin:0; font-weight:600; overflow-wrap:anywhere; }
.meta, .blurb, .note, .links, .metric, figcaption { color:var(--muted); }
.meta, .blurb { margin:0 0 8px; font-size:13px; }
.filters { position:sticky; top:0; background:var(--bg); padding:10px 0;
  border-bottom:1px solid var(--line); z-index:1; display:flex; gap:8px; flex-wrap:wrap; }
.filters button { font:inherit; font-size:13px; padding:4px 12px; border-radius:99px;
  border:1px solid var(--line); background:var(--card); color:var(--fg); cursor:pointer; }
.filters button[aria-pressed=true] { background:var(--fg); color:var(--bg); }
.case { background:var(--card); border:1px solid var(--line); border-radius:10px;
  padding:14px 16px; margin:12px 0; border-left:4px solid var(--line); }
.case.pass { border-left-color:var(--ok); } .case.fail { border-left-color:var(--fail); }
.case.known { border-left-color:var(--known); } .case.xpass { border-left-color:var(--xpass); }
header { display:flex; align-items:baseline; gap:10px; flex-wrap:wrap; }
.links { font-size:12px; margin-left:auto; overflow-wrap:anywhere; }
a { color:inherit; }
.badge { font-size:12px; font-weight:600; padding:1px 8px; border-radius:99px; white-space:nowrap; }
.badge.pass { color:var(--ok); background:var(--ok-bg); }
.badge.fail { color:var(--fail); background:var(--fail-bg); }
.badge.known { color:var(--known); background:var(--known-bg); }
.badge.xpass { color:var(--xpass); background:var(--xpass-bg); }
.note { margin:6px 0 0; font-size:13px; }
.figs { display:flex; gap:12px; flex-wrap:wrap; margin:12px 0 4px; }
figure { margin:0; flex:0 1 180px; min-width:0; }
figure img { display:block; max-width:100%; max-height:220px; border-radius:6px;
  border:1px solid var(--line); background:#fff; }
figure.empty div { height:120px; border:1px dashed var(--line); border-radius:6px;
  display:grid; place-items:center; color:var(--muted); font-size:13px; }
figcaption { font-size:12px; margin-top:4px; }
.why { margin:8px 0 0; padding-left:18px; color:var(--fail); font-size:13px; white-space:pre-wrap; }
.knownwhy, .metric { font-size:13px; margin:8px 0 0; }
details { margin-top:8px; font-size:13px; } summary { cursor:pointer; color:var(--muted); }
pre { white-space:pre-wrap; overflow-wrap:anywhere; font-size:12px; background:var(--bg);
  padding:10px; border-radius:6px; border:1px solid var(--line); }
"""

JS = """
const buttons = document.querySelectorAll('.filters button');
buttons.forEach(b => b.addEventListener('click', () => {
  buttons.forEach(x => x.setAttribute('aria-pressed', x === b));
  const want = b.dataset.show;
  document.querySelectorAll('.case').forEach(c => {
    c.hidden = !(want === 'all' || c.dataset.status === want ||
                 (want === 'attention' && c.dataset.status !== 'pass'));
  });
}));
"""


def load(path):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def main():
    examples = load(os.path.join(OUT, "examples", "results.json"))
    corpus = load(os.path.join(OUT, "corpus", "results.json"))
    if not examples and not corpus:
        sys.exit("no results in tests/out -- run tests/run.sh (or "
                 "tests/regression.py / tests/corpus.py) first")
    rev = subprocess.run(["git", "-C", HERE, "log", "-1", "--format=%h %s"],
                         capture_output=True, text=True).stdout.strip()
    dirty = subprocess.run(["git", "-C", HERE, "status", "--porcelain", "--",
                            "..", ":!tests/corpus", ":!tests/out"],
                           capture_output=True, text=True).stdout.strip()
    when = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
    body = section(
        "Public examples",
        "examples/ — in the repository and under CI. Checked by structural "
        f"facts and against a golden render (goldens: "
        f"{html.escape(examples['flavor']) if examples else '-'}).",
        examples)
    body += section(
        "Local corpus",
        "tests/corpus/ — your own photos and cases.json, never committed. "
        "See README, Testing.", corpus)
    page = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>a4norm test report</title><style>{CSS}</style></head>
<body><main>
<h1>a4norm test report</h1>
<p class=meta>{when} · {html.escape(rev)}{' · uncommitted changes' if dirty else ''}</p>
<nav class=filters>
  <button data-show="all" aria-pressed="true">all</button>
  <button data-show="attention" aria-pressed="false">needs attention</button>
  <button data-show="fail" aria-pressed="false">FAIL</button>
  <button data-show="known" aria-pressed="false">known</button>
  <button data-show="pass" aria-pressed="false">ok</button>
</nav>
{body}
</main><script>{JS}</script></body></html>
"""
    os.makedirs(OUT, exist_ok=True)
    with open(PAGE, "w") as f:
        f.write(page)
    print(f"report: {PAGE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
