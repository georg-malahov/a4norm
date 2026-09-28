#!/usr/bin/env python3
"""The official forms of official.json in examples/forms/official/: check
them, fetch a missing one, or see whether the authorities still publish them.

    python3 examples/forms/official.py [ID ...]             check, fetch the missing
    python3 examples/forms/official.py --online [ID ...]    are they still current?
    python3 examples/forms/official.py --from DIR [ID ...]  take copies from DIR

The files are in the repository as the authorities publish them (see
official/README.md); the list says where each comes from, which version it
is, when it was fetched and its sha256.

Some sites refuse scripts (Berlin answers 429, Frankfurt a Cloudflare
challenge): such a form is reported as unavailable. A copy fetched in a
browser goes in with --from, taken only if its sha256 matches. A form whose
bytes at the URL changed is reported, not taken: the authority has a new
version, and the list and the file are updated by hand.

Exit status: 1 when a file here does not match, or --online found a change.
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
    src, online = None, "--online" in argv
    argv = [a for a in argv if a != "--online"]
    if "--from" in argv:
        i = argv.index("--from")
        src, argv = Path(argv[i + 1]), argv[:i] + argv[i + 2:]
    forms = json.loads((HERE / "official.json").read_text())
    if argv:
        forms = [f for f in forms if f["id"] in argv]
    DIR.mkdir(exist_ok=True)
    bad = 0
    for f in forms:
        path = DIR / f"{f['id']}.pdf"
        if online:
            b, how = fetch(f["url"])
            if b is None:
                print(f"unavailable {f['id']}: {how}")
            elif sha256(b) == f["sha256"]:
                print(f"current     {f['id']}")
            else:
                bad += 1
                print(f"CHANGED     {f['id']}: now sha256 {sha256(b)}, {len(b)} bytes (listed {f['bytes']})")
            continue
        if path.exists():
            if sha256(path.read_bytes()) == f["sha256"]:
                print(f"ok          {f['id']}")
            else:
                bad += 1
                print(f"MISMATCH    {f['id']}: the file here is not the listed one")
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
            bad += 1
            print(f"CHANGED     {f['id']}: sha256 {sha256(b)}, {len(b)} bytes (listed {f['bytes']})")
        else:
            path.write_bytes(b)
            print(f"ok          {f['id']}: {how}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
