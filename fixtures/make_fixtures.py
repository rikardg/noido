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

links.pdf (no text):
  page 0: 612x792 pt with link annotations, in order:
    A  /Dest [page 1 /XYZ 72 600 null]              Rect 72 690 172 710
    B  GoTo (tree.name): /Names /Dests tree, under /Kids, [page 1 /FitH 500]
                                                    Rect 72 660 172 680
    C  /Dest /OldName: catalog /Dests, << /D [page 1 /Fit] >>
                                                    Rect 72 630 172 650
    D  URI https://example.org/ with two /QuadPoints (y 610-620 x 72-300,
       y 580-590 x 72-150)                          Rect 72 580 300 620
    E  GoTo (missing), unresolvable                 Rect 72 550 172 570
    F  GoToR to other.pdf                           Rect 72 520 172 540
    G  /Dest [page 2 /XYZ 100 null 0]               Rect 72 490 172 510
    H  /Dest [page 1 /FitR 100 200 300 400], half off the page
                                                    Rect 500 760 700 800
    I  /Dest [the catalog, not a page /Fit]         Rect 72 460 172 480
    J  a /Text annotation, not a link
    K  /Dest [7 /Fit], a page number past the end   Rect 72 400 172 420
    L  GoTo (sec.1): /Names /Dests tree, [page 1 /XYZ 90 700 null], as
       hyperref makes them                          Rect 72 370 172 390
    M  URI https://example.org/a/long/address, wrapped over two lines:
       M on the first                               Rect 72 340 300 360
    N  and N, the same URI, on the second           Rect 72 310 150 330
  page 1: 612x792 pt, no annotations
  page 2: 595x842 pt with /Rotate 90, no annotations

properties.pdf (PDF 1.7, no text):
  info: /Title (Info Title), /Author "\u00c5sa Olson" in UTF-16BE,
        /Keywords "caf\u00e9 \u2014 menu" in PDFDocEncoding,
        /CreationDate D:20240305142201+01'00', /Producer (make_fixtures.py)
  XMP (uncompressed): dc:title "XMP Title" and dc:creator (losing to info),
        dc:description "XMP subject", xmp:CreatorTool "XMP Writer 1.0" as
        an attribute, xmp:ModifyDate 2024-06-01T09:30:00-04:00
  catalog: /MarkInfo << /Marked true >>, an outline with one item
  fonts:
    page 0: F1 Helvetica (Type1, not embedded), F2 ABCDEF+MinionPro-It
            (Type1, /FontFile3), F3 GHIJKL+NotoSans-Bold (Type0, its
            CIDFontType2 has /FontFile2), F5 a Type3 font with no name;
            form X1, whose fonts are F4 TimesNewRomanPSMT (TrueType, a
            descriptor without a font file) and whose own /XObject has X1
            again
    page 1: F1 and X1 again

shared-names.pdf:
  page 0: 612x792 pt with one link, GoTo (nowhere); the /Names /Dests tree
          is 40 nodes without /Limits, each with the next twice as /Kids:
          2^39 paths through 40 objects
"""

from pathlib import Path


def build_pdf(objects: list[bytes], info: int | None = None) -> bytes:
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
    info_entry = b" /Info %d 0 R" % info if info else b""
    out += b"trailer\n<< /Size %d /Root 1 0 R%s >>\n" % (len(objects) + 1, info_entry)
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


def links() -> bytes:
    annots = [
        b"<< /Type /Annot /Subtype /Link /Rect [72 690 172 710] /Dest [4 0 R /XYZ 72 600 null] >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 660 172 680] /A << /S /GoTo /D (tree.name) >> >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 630 172 650] /Dest /OldName >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 580 300 620]"
        b" /QuadPoints [72 620 300 620 72 610 300 610 72 590 150 590 72 580 150 580]"
        b" /A << /S /URI /URI (https://example.org/) >> >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 550 172 570] /A << /S /GoTo /D (missing) >> >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 520 172 540]"
        b" /A << /S /GoToR /F (other.pdf) /D [0 /Fit] >> >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 490 172 510] /Dest [5 0 R /XYZ 100 null 0] >>",
        b"<< /Type /Annot /Subtype /Link /Rect [500 760 700 800] /Dest [4 0 R /FitR 100 200 300 400] >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 460 172 480] /Dest [1 0 R /Fit] >>",
        b"<< /Type /Annot /Subtype /Text /Rect [72 430 92 450] /Contents (a note) >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 400 172 420] /Dest [7 /Fit] >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 370 172 390] /A << /S /GoTo /D (sec.1) >> >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 340 300 360]"
        b" /A << /S /URI /URI (https://example.org/a/long/address) >> >>",
        b"<< /Type /Annot /Subtype /Link /Rect [72 310 150 330]"
        b" /A << /S /URI /URI (https://example.org/a/long/address) >> >>",
    ]
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R /Names << /Dests 6 0 R >>"
            b" /Dests << /OldName << /D [4 0 R /Fit] >> >> >>",
            b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [%s] >>"
            % b" ".join(annots),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Rotate 90 >>",
            b"<< /Kids [7 0 R] >>",
            b"<< /Limits [(a) (z)] /Names [(sec.1) [4 0 R /XYZ 90 700 null] (tree.name) [4 0 R /FitH 500]] >>",
        ]
    )


def properties() -> bytes:
    xmp = (
        b'<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>\n'
        b'<x:xmpmeta xmlns:x="adobe:ns:meta/">\n'
        b'<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">\n'
        b'<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"'
        b' xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:CreatorTool="XMP Writer 1.0">\n'
        b'<dc:title><rdf:Alt><rdf:li xml:lang="x-default">XMP Title</rdf:li></rdf:Alt></dc:title>\n'
        b"<dc:creator><rdf:Seq><rdf:li>One</rdf:li><rdf:li>Two</rdf:li></rdf:Seq></dc:creator>\n"
        b'<dc:description><rdf:Alt><rdf:li xml:lang="x-default">XMP subject</rdf:li></rdf:Alt></dc:description>\n'
        b"<xmp:ModifyDate>2024-06-01T09:30:00-04:00</xmp:ModifyDate>\n"
        b"</rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>\n"
        b'<?xpacket end="w"?>'
    )
    author = "\u00c5sa Olson".encode("utf-16-be").hex().upper().encode()
    page = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]"
        b" /Resources << /Font << %s >> /XObject << /X1 12 0 R >> >> >>"
    )
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >>"
            b" /Outlines 6 0 R /Metadata 7 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
            page % b"/F1 8 0 R /F2 9 0 R /F3 10 0 R /F5 14 0 R",
            page % b"/F1 8 0 R",
            b"<< /Title (Info Title) /Author <FEFF%s> /Keywords (caf\\351 \\204 menu)"
            b" /CreationDate (D:20240305142201+01'00') /Producer (make_fixtures.py) >>"
            % author,
            b"<< /Type /Outlines /First 15 0 R /Last 15 0 R /Count 1 >>",
            b"<< /Type /Metadata /Subtype /XML /Length %d >>\nstream\n" % len(xmp)
            + xmp
            + b"\nendstream",
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
            b"<< /Type /Font /Subtype /Type1 /BaseFont /ABCDEF+MinionPro-It"
            b" /FontDescriptor << /Type /FontDescriptor /FontName /ABCDEF+MinionPro-It"
            b" /FontFile3 11 0 R >> >>",
            b"<< /Type /Font /Subtype /Type0 /BaseFont /GHIJKL+NotoSans-Bold"
            b" /Encoding /Identity-H /DescendantFonts [<< /Type /Font /Subtype /CIDFontType2"
            b" /BaseFont /GHIJKL+NotoSans-Bold /FontDescriptor << /Type /FontDescriptor"
            b" /FontName /GHIJKL+NotoSans-Bold /FontFile2 11 0 R >> >>] >>",
            b"<< /Length 0 >>\nstream\n\nendstream",
            b"<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources"
            b" << /Font << /F4 13 0 R >> /XObject << /X1 12 0 R >> >> /Length 0 >>"
            b"\nstream\n\nendstream",
            b"<< /Type /Font /Subtype /TrueType /BaseFont /TimesNewRomanPSMT"
            b" /FontDescriptor << /Type /FontDescriptor /FontName /TimesNewRomanPSMT >> >>",
            b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0]"
            b" /CharProcs << >> /Encoding << /Differences [] >> /FirstChar 0 /LastChar 0"
            b" /Widths [0] >>",
            b"<< /Title (One) /Parent 6 0 R /Dest [3 0 R /Fit] >>",
        ],
        info=5,
    )


def shared_names() -> bytes:
    depth = 40
    nodes = [
        b"<< /Kids [%d 0 R %d 0 R] >>" % (5 + i, 5 + i) for i in range(depth - 1)
    ] + [b"<< /Names [(other) [3 0 R /Fit]] >>"]
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R /Names << /Dests 4 0 R >> >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots"
            b" [<< /Type /Annot /Subtype /Link /Rect [72 690 172 710]"
            b" /A << /S /GoTo /D (nowhere) >> >>] >>",
        ]
        + nodes
    )


if __name__ == "__main__":
    here = Path(__file__).resolve().parent
    (here / "two-pages.pdf").write_bytes(two_pages())
    (here / "text.pdf").write_bytes(text())
    (here / "links.pdf").write_bytes(links())
    (here / "shared-names.pdf").write_bytes(shared_names())
    (here / "properties.pdf").write_bytes(properties())
