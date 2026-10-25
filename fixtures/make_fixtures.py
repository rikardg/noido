#!/usr/bin/env python3
"""Generate the PDF test fixtures. Standard library only.

two-pages.pdf:
  page 0: 612x792 pt (Letter) with a black filled square at x 100-200, y 100-200
  page 1: 595x842 pt (A4) with /Rotate 90, blank

text.pdf:
  page 0: 612x792 pt (Letter), Helvetica 12 pt (not embedded): "Hello World"
          with its baseline starting at (72, 700) in PDF space, then "noido"
          20 pt lower
  page 1: the same page with /Rotate 90
"""

from pathlib import Path


def build_pdf(objects: list[bytes]) -> bytes:
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n" % (len(objects) + 1)
    out += b"0000000000 65535 f \n"
    for offset in offsets:
        out += b"%010d 00000 n \n" % offset
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (len(objects) + 1)
    out += b"startxref\n%d\n%%%%EOF\n" % xref
    return bytes(out)


def two_pages() -> bytes:
    content = b"0 0 0 rg 100 100 100 100 re f"
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>",
            b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Rotate 90 >>",
        ]
    )


def text() -> bytes:
    content = b"BT /F1 12 Tf 72 700 Td (Hello World) Tj 0 -20 Td (noido) Tj ET"
    page = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R"
        b" /Resources << /Font << /F1 4 0 R >> >>%s >>"
    )
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>",
            page % b"",
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica"
            b" /Encoding /WinAnsiEncoding >>",
            b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
            page % b" /Rotate 90",
        ]
    )


if __name__ == "__main__":
    here = Path(__file__).resolve().parent
    (here / "two-pages.pdf").write_bytes(two_pages())
    (here / "text.pdf").write_bytes(text())
