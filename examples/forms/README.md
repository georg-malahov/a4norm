# Form fixtures

A German patient intake form ("Patientendaten und Datenschutzerklärung") from a demo
service, laid out like a real MRI practice's form. The practice ("Ihr MRT Praxis,
Musterstadt") and the patient are made up; the values come from a fake-data generator. The
phone number is a Bundesnetzagentur "drama number" (Berlin 030 23125 000–999), which is never
given to anyone.

One blank page in every shape a form reaches A4Norm Forms in. Every shape is read the same
way, as a picture of the page, so all of them must give the same fields.

| File | What it is | Where it comes from |
|---|---|---|
| `demo-filled.pdf` | The filled form as its app printed it: a text PDF, one A4 page | the demo service (react-pdf) |
| `demo-blank.pdf` | The same page with the 12 values removed; dotted lines and boxes untouched | `demo-filled.pdf`, values taken out |
| `demo-blank-scan.jpg` | The blank page as a scan: 200 dpi, turned 0.6°, noise, JPEG | rendered from `demo-blank.pdf` |
| `demo-filled-scan.jpg` | The filled page, scanned the same way | rendered from `demo-filled.pdf` |
| `demo-blank-a4norm-scan.pdf` | A real scan: `demo-blank.pdf` printed on paper and taken with the A4Norm scanner on a phone. JPEG 1654 × 2339 in a PDF, no text layer, no metadata, no EXIF | the A4Norm web app, 2026-09-28, as it came out |
| `demo-blank-image.pdf` | `demo-blank-scan.jpg` as the one image of an A4 page | `make.py` |
| `demo-blank-acroform.pdf` | `demo-blank.pdf` plus a form: a text field on each of the 20 writing lines (`undefined`, `undefined_1` …, `DA /Helv 0 Tf` blue) and a check box on each of the 4 boxes | `make.py` |
| `demo-blank-encrypted.pdf` | `demo-blank.pdf`, AES-128: an empty user password, and an owner password that forbids changes and copying (like the Familienkasse's forms and Bavaria's Wohngeld) | `make.py` |
| `demo-truth.json` | The 12 values and their boxes in points (A4 = 595.28 × 841.89) | `demo-filled.pdf`'s text |
| `demo-filled.txt`, `demo-blank.txt` | The text of the two PDFs, a line of the page per line: the truth OCR is checked against (a4norm-ocr's tests) | pypdf's extraction, one kerning split (`MRT -Voruntersuchung`) mended |

The synthetic scans are made from the PDFs, so the truth boxes hold for them after the same
0.6° turn about the page centre (200 dpi: 1 pt = 200/72 px). Their renderer set the bold
paragraphs in the regular face; the real scan shows them bold. The real scan is the page as
the scanner flattened and fitted it, so its boxes are close but not exact.

`make.py` writes the three `demo-blank-*.pdf` variants again (`pip install pypdf
cryptography`). The encrypted one gets a new random file ID each time.

All of them open in pdf.js, in PDFKit (Preview, Safari) and in poppler; the encrypted one
without asking for a password.

## Official forms: linked, not committed

`official.json` lists 14 forms of German authorities: what each is, who publishes it, the
version, when it was fetched, its URL, size and sha256. `official.py` fetches them into
`official/` (git-ignored) and checks the sha256:

```sh
python3 examples/forms/official.py                      # all of them
python3 examples/forms/official.py ba-kg1-kindergeld
python3 examples/forms/official.py --from ~/Downloads   # copies fetched in a browser
```

Authorities' forms are free to use (§ 5 Abs. 2 UrhG) but not to change, and they are
replaced by new versions, so the repository keeps where they are rather than the files.
Frankfurt (a Cloudflare check) and Berlin (429) refuse scripts: the script reports them as
unavailable, a test that needs them skips, and a copy fetched in a browser goes in with
`--from`, taken only if its sha256 matches. A form whose bytes changed is reported and not
taken; the list is then updated by hand.
