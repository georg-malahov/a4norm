#!/usr/bin/env python3
"""Make the demo form's PDF variants from demo-blank.pdf and demo-blank-scan.jpg.

    python3 examples/forms/make.py        (needs: pip install pypdf cryptography)

  demo-blank-image.pdf      the scan as the one image of an A4 page
  demo-blank-acroform.pdf   demo-blank.pdf plus a form: a text field on every
                            writing line, named undefined_N, DA 0 Tf blue, and a
                            check box on every box
  demo-blank-encrypted.pdf  demo-blank.pdf, AES-128, an empty user password and
                            an owner password that forbids changes and copying

The three hold the same page as demo-blank.pdf, so reading them must give the
same fields: a form's structure and its encryption are not what A4Norm Forms
reads. The encrypted file has a random file ID, so each run writes other bytes.
"""

import re
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.constants import UserAccessPermissions as P
from pypdf.generic import (ArrayObject, BooleanObject, DecodedStreamObject, DictionaryObject,
                           FloatObject, NameObject, NumberObject, TextStringObject)

HERE = Path(__file__).resolve().parent
A4 = (595.28, 841.89)


def image_pdf(jpg: Path, out: Path) -> None:
    """One A4 page showing `jpg` edge to edge, the JPEG bytes as they are."""
    data = jpg.read_bytes()
    w, h = jpeg_size(data)
    content = f"q {A4[0]} 0 0 {A4[1]} 0 0 cm /Im0 Do Q".encode()
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {A4[0]} {A4[1]}] "
        f"/Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>".encode(),
        f"<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB "
        f"/BitsPerComponent 8 /Filter /DCTDecode /Length {len(data)} >>\nstream\n".encode()
        + data + b"\nendstream",
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"\nendstream",
    ]
    pdf, offsets = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"), []
    for i, o in enumerate(objs, 1):
        offsets.append(len(pdf))
        pdf += f"{i} 0 obj\n".encode() + o + b"\nendobj\n"
    xref = len(pdf)
    pdf += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
    pdf += b"".join(f"{o:010d} 00000 n \n".encode() for o in offsets)
    pdf += f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    out.write_bytes(pdf)


def jpeg_size(data: bytes) -> tuple[int, int]:
    i = 2
    while i < len(data):
        marker, length = data[i + 1], int.from_bytes(data[i + 2:i + 4], "big")
        if marker in (0xC0, 0xC1, 0xC2):
            return int.from_bytes(data[i + 7:i + 9], "big"), int.from_bytes(data[i + 5:i + 7], "big")
        i += 2 + length
    raise ValueError("no SOF in the JPEG")


def page_marks(page) -> tuple[list, list]:
    """The writing lines and the check boxes the page draws, in PDF points
    (origin bottom left): lines as (x0, y, x1), boxes as (x0, y0, x1, y1).

    react-pdf draws every element in its own `q 1 0 0 -1 X Y cm` frame: a
    line is a W x 1 clip then a 2 pt stroke, a box a 20 x 20 square."""
    c = page.get_contents().get_data().decode("latin-1")
    lines = [(float(x), float(y) - 1, float(x) + float(w))
             for x, y, w in re.findall(r"q 1 0 0 -1 ([\d.]+) ([\d.]+) cm ([\d.]+) 1 m 0 1 l 0 0 l", c)]
    boxes = [(float(x), float(y) - 20, float(x) + 20, float(y))
             for x, y in re.findall(r"q 1 0 0 -1 ([\d.]+) ([\d.]+) cm 0 0 m 20 0 l 20 20 l 0 20 l 0 0 l S", c)]
    return lines, boxes


def appearance(w: float, h: float, ops: str) -> DecodedStreamObject:
    s = DecodedStreamObject()
    s.set_data(ops.encode())
    s.update({NameObject("/Type"): NameObject("/XObject"), NameObject("/Subtype"): NameObject("/Form"),
              NameObject("/BBox"): ArrayObject([FloatObject(0), FloatObject(0), FloatObject(w), FloatObject(h)])})
    return s


def acroform_pdf(src: Path, out: Path) -> None:
    """Fields the way the forms in the wild have them: generic names, a blue
    auto-sized font, rectangles a little off the lines they sit on."""
    w = PdfWriter(clone_from=src)
    page = w.pages[0]
    lines, boxes = page_marks(page)
    helv = w._add_object(DictionaryObject({
        NameObject("/Type"): NameObject("/Font"), NameObject("/Subtype"): NameObject("/Type1"),
        NameObject("/BaseFont"): NameObject("/Helvetica"), NameObject("/Encoding"): NameObject("/WinAnsiEncoding")}))
    fields, n = ArrayObject(), 0

    def add(d: dict) -> None:
        nonlocal n
        d.update({NameObject("/Type"): NameObject("/Annot"), NameObject("/Subtype"): NameObject("/Widget"),
                  NameObject("/T"): TextStringObject(f"undefined_{n}" if n else "undefined"),
                  NameObject("/F"): NumberObject(4), NameObject("/P"): page.indirect_reference})
        n += 1
        ref = w._add_object(DictionaryObject(d))
        fields.append(ref)
        page.setdefault(NameObject("/Annots"), ArrayObject()).append(ref)

    for x0, y, x1 in sorted(lines, key=lambda l: (-l[1], l[0])):
        add({NameObject("/FT"): NameObject("/Tx"),
             NameObject("/Rect"): ArrayObject([FloatObject(v) for v in (x0, y + 1, x1, y + 17)]),
             NameObject("/DA"): TextStringObject("/Helv 0 Tf 0 0 1 rg")})
    for x0, y0, x1, y1 in sorted(boxes, key=lambda b: (-b[1], b[0])):
        s = x1 - x0
        on = appearance(s, s, f"q 0 0 1 rg BT /ZaDb {s * 0.8:.1f} Tf {s * 0.15:.1f} {s * 0.2:.1f} Td (4) Tj ET Q")
        on[NameObject("/Resources")] = DictionaryObject({NameObject("/Font"): DictionaryObject({
            NameObject("/ZaDb"): w._add_object(DictionaryObject({
                NameObject("/Type"): NameObject("/Font"), NameObject("/Subtype"): NameObject("/Type1"),
                NameObject("/BaseFont"): NameObject("/ZapfDingbats")}))})})
        add({NameObject("/FT"): NameObject("/Btn"),
             NameObject("/Rect"): ArrayObject([FloatObject(v) for v in (x0, y0, x1, y1)]),
             NameObject("/DA"): TextStringObject("/ZaDb 0 Tf 0 0 1 rg"),
             NameObject("/MK"): DictionaryObject({NameObject("/CA"): TextStringObject("4")}),
             NameObject("/V"): NameObject("/Off"), NameObject("/AS"): NameObject("/Off"),
             NameObject("/AP"): DictionaryObject({NameObject("/N"): DictionaryObject({
                 NameObject("/Yes"): w._add_object(on),
                 NameObject("/Off"): w._add_object(appearance(s, s, ""))})})})
    w._root_object[NameObject("/AcroForm")] = DictionaryObject({
        NameObject("/Fields"): fields,
        NameObject("/DA"): TextStringObject("/Helv 0 Tf 0 g"),
        NameObject("/NeedAppearances"): BooleanObject(False),
        NameObject("/DR"): DictionaryObject({NameObject("/Font"): DictionaryObject({NameObject("/Helv"): helv})})})
    with out.open("wb") as f:
        w.write(f)
    print(f"{out.name}: {len(lines)} text fields, {len(boxes)} check boxes")


def encrypted_pdf(src: Path, out: Path) -> None:
    """Opens without a password; the owner password forbids changing and
    copying, like the Familienkasse's forms and Bavaria's Wohngeld."""
    # page by page: a clone keeps the source's indirect Info strings, and
    # pypdf then encrypts a reference to an object it never writes
    r, w = PdfReader(src), PdfWriter()
    w.add_page(r.pages[0])
    w.add_metadata({k: str(v) for k, v in r.metadata.items()})
    w.encrypt(user_password="", owner_password="a4norm-demo-owner", algorithm="AES-128",
              permissions_flag=P.PRINT | P.PRINT_TO_REPRESENTATION)
    with out.open("wb") as f:
        w.write(f)


if __name__ == "__main__":
    blank = HERE / "demo-blank.pdf"
    image_pdf(HERE / "demo-blank-scan.jpg", HERE / "demo-blank-image.pdf")
    acroform_pdf(blank, HERE / "demo-blank-acroform.pdf")
    encrypted_pdf(blank, HERE / "demo-blank-encrypted.pdf")
    for n in ("demo-blank-image.pdf", "demo-blank-acroform.pdf", "demo-blank-encrypted.pdf"):
        print(f"{n}: {(HERE / n).stat().st_size} bytes")
