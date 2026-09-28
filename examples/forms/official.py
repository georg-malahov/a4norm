#!/usr/bin/env python3
"""Fetch the official forms of official.json into examples/forms/official/
(git-ignored) and check each against its sha256.

    python3 examples/forms/official.py [--from DIR] [ID ...]

The forms are not in the repository: authorities' forms are free to use
(§ 5 UrhG) but not to change, and they are updated. The list says where each
one comes from, which version it was, when it was fetched and its sha256.

Some sites refuse scripts (Berlin answers 429, Frankfurt a Cloudflare
challenge). Such a form is reported as unavailable and a test that needs it
skips; a copy fetched in a browser can be put in with --from, and it is taken
only if its sha256 matches. A form whose bytes changed is reported, not taken:
the authority has a new version, and the list needs updating by hand.

Exit status: 0 when every form asked for is here and matches (unavailable
ones included in that only if a copy is already here), 1 when one changed.
"""

import hashlib
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
DIR = HERE / "official"
UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15"


def sha256(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def fetch(url: str) -> tuple[bytes | None, str]:
    try:
        req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "application/pdf,*/*"})
        with urllib.request.urlopen(req, timeout=60) as r:
            b = r.read()
    except urllib.error.HTTPError as e:
        return None, f"HTTP {e.code}"
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        return None, f"network: {getattr(e, 'reason', e)}"
    if not b.startswith(b"%PDF"):
        return None, "not a PDF (a bot check?)"
    return b, "fetched"


def main(argv: list[str]) -> int:
    src = None
    if "--from" in argv:
        i = argv.index("--from")
        src, argv = Path(argv[i + 1]), argv[:i] + argv[i + 2:]
    forms = json.loads((HERE / "official.json").read_text())
    if argv:
        forms = [f for f in forms if f["id"] in argv]
    DIR.mkdir(exist_ok=True)
    changed = 0
    for f in forms:
        path = DIR / f"{f['id']}.pdf"
        if path.exists() and sha256(path.read_bytes()) == f["sha256"]:
            print(f"ok          {f['id']}")
            continue
        b, how = None, "no copy given"
        if src:
            given = [p for p in src.glob("*.pdf") if sha256(p.read_bytes()) == f["sha256"]]
            if given:
                b, how = given[0].read_bytes(), f"from {given[0].name}"
        if b is None:
            b, how = fetch(f["url"])
        if b is None:
            print(f"unavailable {f['id']}: {how}")
        elif sha256(b) != f["sha256"]:
            changed += 1
            print(f"CHANGED     {f['id']}: sha256 {sha256(b)}, {len(b)} bytes (listed {f['bytes']})")
        else:
            path.write_bytes(b)
            print(f"ok          {f['id']}: {how}")
    return 1 if changed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
