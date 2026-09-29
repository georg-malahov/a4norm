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
| `demo-template.json`, `demo-answers.json` | A template of the form as the model's structure gives it (fields on the candidates' numbers; one drawn as a box), and the 12 values plus two choices | written by hand from the geometry's numbers |
| `kg1-p2-template.json`, `kg1-p2-answers.json` | KG 1's page 2 (`official/`) as a template on its candidates, and made-up answers: the tax ID over its four combs, a test IBAN (DE02120300000000202051), boxes | written by hand |
| `demo-filled.txt`, `demo-blank.txt` | The text of the two PDFs, a line of the page per line: the truth OCR is checked against (a4norm-ocr's tests) | pypdf's extraction, one kerning split (`MRT -Voruntersuchung`) mended |

The synthetic scans are made from the PDFs, so the truth boxes hold for them after the same
0.6° turn about the page centre (200 dpi: 1 pt = 200/72 px). Their renderer set the bold
paragraphs in the regular face; the real scan shows them bold. The real scan is the page as
the scanner flattened and fitted it, so its boxes are close but not exact.

`make.py` writes the three `demo-blank-*.pdf` variants again (`pip install pypdf
cryptography`). The encrypted one gets a new random file ID each time.

All of them open in pdf.js, in PDFKit (Preview, Safari) and in poppler; the encrypted one
without asking for a password.

## Official forms

`official/` holds 14 blank forms of German authorities, byte for byte as they publish them.
They are **not under the MIT licence**: amtliche Werke (§ 5 Abs. 2 UrhG), downloaded from
public sources for tests. `official/README.md` names for each the authority, the source URL,
the version, the download date and the sha256.

`official.json` lists the same, and `official.py` checks them:

```sh
python3 examples/forms/official.py                      # the files here match the list
python3 examples/forms/official.py --online             # the authorities still publish these versions
python3 examples/forms/official.py --from ~/Downloads   # a missing one, from copies fetched in a browser
```

Frankfurt (a Cloudflare check) and Berlin (429) refuse scripts and show as unavailable
online. A form whose bytes at its URL changed is reported and not taken; the list and the
file are then updated by hand, and a form that carries a form publisher's imprint or
"Nachdruck verboten" stays a link in the list only.
