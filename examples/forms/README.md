# Form fixtures

A German patient intake form ("Patientendaten und Datenschutzerklärung") from a demo
service, laid out like a real MRI practice's form. The practice ("Ihr MRT Praxis,
Musterstadt") and the patient are made up; the values come from a fake-data generator.

| File | What it is |
|---|---|
| `demo-filled.pdf` | The filled form as its app printed it: a text PDF, one A4 page |
| `demo-blank.pdf` | The same page with the 12 values removed; dotted lines and boxes untouched |
| `demo-blank-scan.jpg` | The blank page as a scan: 200 dpi, turned 0.6°, noise, JPEG |
| `demo-filled-scan.jpg` | The filled page, scanned the same way |
| `demo-truth.json` | The 12 values and their boxes in points (A4 = 595.28 × 841.89) |

The scans are made from the PDFs, so the truth boxes hold for them after the same 0.6°
turn about the page centre (200 dpi: 1 pt = 200/72 px).
