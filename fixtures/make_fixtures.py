#!/usr/bin/env python3
"""Generate the PDF and CBZ test fixtures. Standard library only.

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

black.pdf:
  page 0: 612x792 pt (Letter), filled black edge to edge
  page 1: the same page with /Rotate 90

type3.pdf:
  page 0: 612x792 pt (Letter), one Type 3 font whose glyphs are content
          streams: each of "a", "b" and "c" is a quarter em wide
          (FontMatrix 0.001, /Widths 250), drawn at 12 pt starting at (72, 700)

tiling.pdf:
  page 0: 612x792 pt, filled with a tiling pattern of one 10x10 pt red cell
          repeated every 99999 pt: a tile far smaller than its step, so only
          one tile shows. Sizing a tile by its step overflows a 16-bit
          dimension (hayro 0.8 asked vello for a 65535-px square and
          panicked on every page like it)

shared-names.pdf:
  page 0: 612x792 pt with one link, GoTo (nowhere); the /Names /Dests tree
          is 40 nodes without /Limits, each with the next twice as /Kids:
          2^39 paths through 40 objects

page-labels.pdf: 12 blank pages; /PageLabels is a number tree with /Kids:
  first kid: 1 lower roman, 3 prefix "A " (its space kept) decimal from 5, 5 prefix "Cover"
             alone, 7 an empty range
  second kid: 8 upper roman from 4, 9 lower letters from 27, 10 upper letters
              with /St 0, 11 decimal; and /Kids back to the root, a cycle
  page 0 comes before the first range

outline.pdf: 5 pages at 612x792 pt with this outline:
  Chapter 1      /Dest [p0 /XYZ 72 700 null], /Count 2
    Section 1.1  /A GoTo (sec), in the /Names tree, to [p1 /FitH 600], /Count 1
      Deep       /Dest [p2 /Fit]
    Section 1.2  /Dest [p0 /XYZ 72 700 null]
  Chapter 2      /A URI https://example.org/, /Count -1
    Hidden       /Dest [p3 /Fit]
  Part           no target, /Count 1
    Unresolved   /Dest (nowhere)
  "\u00c5sa" in UTF-16BE, /Dest [p4 /Fit]
  an empty title, no target
  "Two\n  lines", /A GoToR, which noido doesn't follow
  Cycle A        /Dest [p0 /Fit], no /Count, /Next Cycle B
  Cycle B        /Next Cycle A (a /Next cycle), /First Chapter 1 (a /First cycle)

outline-empty.pdf: one page; /Outlines has no /First

outline-deep.pdf: one page; 70 outline items, each the only child of the one
  before

mixed-sizes.pdf: four blank pages: three of 595x842 pt and one of 1190x1684 pt
  (twice the size, so scale to fit shrinks it to half)

comic.cbz: four pages in "pages/", named 1, 2, 10 and 11, so natural order
  differs from lexical; 10 is 20x30 px, the others 10x15. ComicInfo.xml has
  a title, writer, summary, genre and date; notes.txt and a __MACOSX entry
  are there to be ignored.

comic-deflated.cbz: the same pages and metadata, deflated rather than stored.
comic-alpha.cbz: one 2x2 RGBA page, 200/100/50 at alpha 128.
comic-broken.cbz: a good page, then a page whose IDAT is corrupt.
comic-damaged.cbz: a good page, then a PNG signature with no IHDR.
comic-duplicate.cbz: two entries named 1.png; the ZIP reader keeps one.
comic-empty.cbz: ComicInfo and notes, no pages.
comic-huge.cbz: an 8000x8000 IHDR with no page data.
comic-bad-info.cbz: an unclosed ComicInfo title.
"""

import struct
import zipfile
import zlib
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


def tj() -> bytes:
    # A letter-spaced heading in one TJ array, as InDesign writes them,
    # then a plain Tj.
    content = (
        b"BT /F1 1 Tf 0.036 Tc 0.014 Tw 10.5 0 0 10.5 72 700 Tm"
        b" [(E)-69.6 (AG)-17.6 (L)-48.1 (E)-14 ( E)-51 (Y)-73.4 (E)] TJ"
        b" 0 Tc 0 Tw 0 -2 Td (next) Tj ET"
    )
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R"
            b" /Resources << /Font << /F1 4 0 R >> >> >>",
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica"
            b" /Encoding /WinAnsiEncoding >>",
            b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
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


def black() -> bytes:
    content = b"0 0 0 rg 0 0 612 792 re f"
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>",
            b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate 90"
            b" /Contents 4 0 R >>",
        ]
    )


def mixed_sizes() -> bytes:
    page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] >>"
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R] /Count 4 >>",
            page % (595, 842),
            page % (595, 842),
            page % (595, 842),
            page % (1190, 1684),
        ]
    )


def type3() -> bytes:
    char_proc = b"250 0 d0"
    content = b"BT /F1 12 Tf 72 700 Td (abc) Tj ET"
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 6 0 R"
            b" /Resources << /Font << /F1 4 0 R >> >> >>",
            b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]"
            b" /FontMatrix [0.001 0 0 0.001 0 0]"
            b" /CharProcs << /a 5 0 R /b 5 0 R /c 5 0 R >>"
            b" /Encoding << /Type /Encoding /Differences [97 /a /b /c] >>"
            b" /FirstChar 97 /LastChar 99 /Widths [250 250 250]"
            b" /Resources << >> >>",
            b"<< /Length %d >>\nstream\n" % len(char_proc) + char_proc + b"\nendstream",
            b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
        ]
    )


def tiling() -> bytes:
    cell = b"1 0 0 rg 0 0 10 10 re f"
    content = b"/Pattern cs /P1 scn 0 0 612 792 re f"
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R"
            b" /Resources << /Pattern << /P1 4 0 R >> >> >>",
            b"<< /Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1"
            b" /BBox [0 0 10 10] /XStep 99999 /YStep 99999 /Resources << >>"
            b" /Length %d >>\nstream\n"
            % len(cell) + cell + b"\nendstream",
            b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
        ]
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


def page_labels() -> bytes:
    pages = range(12)
    kids = b" ".join(b"%d 0 R" % (3 + page) for page in pages)
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R /PageLabels 15 0 R >>",
            b"<< /Type /Pages /Kids [%s] /Count 12 >>" % kids,
        ]
        + [b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>" for _ in pages]
        + [
            b"<< /Kids [16 0 R 17 0 R] >>",
            b"<< /Limits [1 7] /Nums [1 << /S /r >> 3 << /P (A ) /S /D /St 5 >>"
            b" 5 << /P (Cover) >> 7 << >>] >>",
            b"<< /Limits [8 11] /Nums [8 << /S /R /St 4 >> 9 << /S /a /St 27 >>"
            b" 10 << /S /A /St 0 >> 11 << /S /D >>] /Kids [15 0 R] >>",
        ]
    )


def outline() -> bytes:
    first = 9  # The object number of the first outline item.
    # Name, then the dictionary's other entries, by name of what they link to.
    items = [
        ("ch1", b"(Chapter 1)", b"/Dest [3 0 R /XYZ 72 700 null] /Count 2", "root", {"First": "sec11", "Next": "ch2"}),
        ("sec11", b"(Section 1.1)", b"/A << /S /GoTo /D (sec) >> /Count 1", "ch1", {"First": "deep", "Next": "sec12"}),
        ("deep", b"(Deep)", b"/Dest [5 0 R /Fit]", "sec11", {}),
        ("sec12", b"(Section 1.2)", b"/Dest [3 0 R /XYZ 72 700 null]", "ch1", {}),
        ("ch2", b"(Chapter 2)", b"/A << /S /URI /URI (https://example.org/) >> /Count -1", "root", {"First": "hidden", "Next": "part"}),
        ("hidden", b"(Hidden)", b"/Dest [6 0 R /Fit]", "ch2", {}),
        ("part", b"(Part)", b"/Count 1", "root", {"First": "unresolved", "Next": "asa"}),
        ("unresolved", b"(Unresolved)", b"/Dest (nowhere)", "part", {}),
        ("asa", b"<FEFF00C500730061>", b"/Dest [7 0 R /Fit]", "root", {"Next": "empty"}),
        ("empty", b"()", b"", "root", {"Next": "two"}),
        ("two", b"(Two\\n  lines)", b"/A << /S /GoToR /F (other.pdf) /D [0 /Fit] >>", "root", {"Next": "cycle_a"}),
        ("cycle_a", b"(Cycle A)", b"/Dest [3 0 R /Fit]", "root", {"Next": "cycle_b"}),
        ("cycle_b", b"(Cycle B)", b"", "root", {"Next": "cycle_a", "First": "ch1"}),
    ]
    number = {item[0]: first + i for i, item in enumerate(items)} | {"root": 8}
    bodies = [
        b"<< /Title %s %s /Parent %d 0 R %s >>"
        % (
            title,
            rest,
            number[parent],
            b" ".join(
                b"/%s %d 0 R" % (key.encode(), number[to]) for key, to in links.items()
            ),
        )
        for _, title, rest, parent, links in items
    ]
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R /Outlines 8 0 R"
            b" /Names << /Dests << /Names [(sec) [4 0 R /FitH 600]] >> >> >>",
            b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R 7 0 R] /Count 5 >>",
        ]
        + [b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>"] * 5
        + [b"<< /Type /Outlines /First %d 0 R /Last %d 0 R >>" % (first, number["cycle_b"])]
        + bodies
    )


def outline_empty() -> bytes:
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
            b"<< /Type /Outlines /Count 0 >>",
        ]
    )


def outline_deep() -> bytes:
    depth = 70
    first = 5
    items = [
        b"<< /Title (Level %d) /Dest [3 0 R /Fit] /Count 1 /Parent %d 0 R%s >>"
        % (
            i,
            first + i - 1 if i else 4,
            b" /First %d 0 R" % (first + i + 1) if i + 1 < depth else b"",
        )
        for i in range(depth)
    ]
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
            b"<< /Type /Outlines /First %d 0 R >>" % first,
        ]
        + items
    )


def comic_png_chunk(kind: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data))
        + kind
        + data
        + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    )


def comic_png_with(width: int, height: int, color_type: int, pixel: bytes) -> bytes:
    """A PNG of one solid colour, standard library only."""
    raw = (b"\x00" + pixel * width) * height
    return (
        b"\x89PNG\r\n\x1a\n"
        + comic_png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, color_type, 0, 0, 0))
        + comic_png_chunk(b"IDAT", zlib.compress(raw))
        + comic_png_chunk(b"IEND", b"")
    )


def comic_png(width: int, height: int, rgb: tuple[int, int, int]) -> bytes:
    return comic_png_with(width, height, 2, bytes(rgb))


def comic_png_rgba(width: int, height: int, rgba: tuple[int, int, int, int]) -> bytes:
    return comic_png_with(width, height, 6, bytes(rgba))


def comic_info() -> str:
    return (
        "<ComicInfo>"
        "<Title>The Test Comic</Title>"
        "<Writer>A. Writer</Writer>"
        "<Summary>Four pages, out of order.</Summary>"
        "<Genre>Test</Genre>"
        "<Year>2026</Year><Month>11</Month><Day>15</Day>"
        "</ComicInfo>"
    )


def comic(compression: int = zipfile.ZIP_STORED) -> bytes:
    """A CBZ whose page names sort naturally, with metadata and junk."""
    import io

    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", compression) as z:
        z.writestr("ComicInfo.xml", comic_info())
        z.writestr("pages/1.png", comic_png(10, 15, (255, 0, 0)))
        z.writestr("pages/2.png", comic_png(10, 15, (0, 255, 0)))
        z.writestr("pages/10.png", comic_png(20, 30, (0, 0, 255)))
        z.writestr("pages/11.png", comic_png(10, 15, (255, 255, 255)))
        z.writestr("notes.txt", "not a page")
        z.writestr("__MACOSX/._1.png", b"\x00\x05\x16\x07AppleDouble junk")
    return out.getvalue()


def comic_alpha() -> bytes:
    """One 2x2 RGBA page: 200, 100, 50 at half alpha."""
    import io

    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_STORED) as z:
        z.writestr("1.png", comic_png_rgba(2, 2, (200, 100, 50, 128)))
    return out.getvalue()


def comic_broken() -> bytes:
    """ComicInfo, a good first page, then a page whose IDAT is corrupt."""
    import io

    broken = bytearray(comic_png(10, 15, (0, 255, 0)))
    # IHDR is 8 signature + 25 bytes; IDAT's data starts at 41.
    broken[45] ^= 0xFF
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_STORED) as z:
        z.writestr("ComicInfo.xml", comic_info())
        z.writestr("1.png", comic_png(10, 15, (255, 0, 0)))
        z.writestr("2.png", bytes(broken))
    return out.getvalue()


def comic_duplicate() -> bytes:
    """Two entries with one name: the reader keeps one page."""
    import io
    import warnings

    out = io.BytesIO()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        with zipfile.ZipFile(out, "w", zipfile.ZIP_STORED) as z:
            z.writestr("1.png", comic_png(10, 15, (255, 0, 0)))
            z.writestr("1.png", comic_png(10, 15, (0, 255, 0)))
    return out.getvalue()


def comic_damaged() -> bytes:
    """A good first page, then a page with a valid PNG signature and no IHDR."""
    import io

    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_STORED) as z:
        z.writestr("1.png", comic_png(10, 15, (255, 0, 0)))
        z.writestr("2.png", b"\x89PNG\r\n\x1a\n\x00\x00")
    return out.getvalue()


def comic_empty() -> bytes:
    """Metadata and notes, but no page at all."""
    import io

    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_STORED) as z:
        z.writestr("ComicInfo.xml", comic_info())
        z.writestr("notes.txt", "not a page")
    return out.getvalue()


def comic_huge() -> bytes:
    """A page whose IHDR says 8000x8000, with no data to decode."""
    import io

    ihdr = struct.pack(">IIBBBBB", 8000, 8000, 8, 2, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + comic_png_chunk(b"IHDR", ihdr)
        + comic_png_chunk(b"IDAT", zlib.compress(b"\x00\x00\x00\x00"))
        + comic_png_chunk(b"IEND", b"")
    )
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_STORED) as z:
        z.writestr("1.png", png)
    return out.getvalue()


def comic_bad_info() -> bytes:
    """A page with an unclosed ComicInfo title."""
    import io

    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_STORED) as z:
        z.writestr("ComicInfo.xml", "<ComicInfo><Title>unclosed")
        z.writestr("1.png", comic_png(10, 15, (255, 0, 0)))
    return out.getvalue()


EPUB_NS = (
    'xmlns="http://www.w3.org/1999/xhtml" '
    'xmlns:epub="http://www.idpf.org/2007/ops"'
)


def epub_xhtml(body: str) -> str:
    """A chapter as real books have it: a DOCTYPE and indented structure."""
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        "<!DOCTYPE html>\n"
        f"<html {EPUB_NS}>\n"
        "  <head>\n    <title>t</title>\n  </head>\n"
        f"  <body>\n{body}\n  </body>\n</html>\n"
    )


def epub_zip(files: list[tuple[str, bytes | str]], package_path: str) -> bytes:
    """An EPUB: mimetype first and stored, then the container, then `files`."""
    import io

    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("mimetype", "application/epub+zip", zipfile.ZIP_STORED)
        z.writestr(
            "META-INF/container.xml",
            '<?xml version="1.0"?>'
            '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
            f'<rootfiles><rootfile full-path="{package_path}" '
            'media-type="application/oebps-package+xml"/></rootfiles></container>',
        )
        for name, data in files:
            z.writestr(name, data)
    return out.getvalue()


def epub_book() -> bytes:
    ch1 = (
        "<h1>Chapter One</h1>"
        "<p>It was a <i>dark</i> and <b>stormy</b> night.</p>"
        '<p>See <a href="ch%202.xhtml#later">later</a>, and H<sub>2</sub>O<sup>1</sup>.</p>'
        "<hr/><ul><li>first</li><li>second</li></ul>"
        "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>"
        "<pre>  two  spaces</pre>"
        '<p><img src="../images/f0001-01.png" alt="f0001-01"/></p>'
    )
    ch2 = (
        '<h1 id="top">Chapter Two</h1><p id="later">Later text.</p>'
        '<p>Back to <a href="ch1.xhtml">the start</a>, '
        '<a href="https://example.com/">the web</a> and '
        '<a href="../cover.png">a picture</a>.</p>'
    )
    nav = (
        '<nav epub:type="toc"><ol>'
        '<li><a href="text/ch1.xhtml">Chapter&nbsp;One</a></li>'
        '<li><a href="text/ch%202.xhtml#top">\n  Chapter\n  Two </a>'
        '<ol><li><a href="text/ch%202.xhtml#later">Later</a></li></ol></li>'
        '<li><a href="nowhere.xhtml">Missing</a></li>'
        "</ol></nav>"
    )
    opf = (
        '<?xml version="1.0"?>'
        '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">'
        '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">'
        '<dc:identifier id="id">urn:uuid:1</dc:identifier>'
        "<dc:title>A Small Book</dc:title>"
        "<dc:creator>Ann Author</dc:creator><dc:creator>Bo Writer</dc:creator>"
        "<dc:language>sv</dc:language><dc:subject>Tests</dc:subject>"
        "</metadata><manifest>"
        '<item id="cover" href="cover.xhtml" media-type="application/xhtml+xml"/>'
        '<item id="coverimg" href="cover.png" media-type="image/png" properties="cover-image"/>'
        '<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
        '<item id="ch1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>'
        '<item id="ch2" href="text/ch%202.xhtml" media-type="application/xhtml+xml"/>'
        '</manifest><spine><itemref idref="cover"/><itemref idref="ch1"/>'
        '<itemref idref="ch2"/></spine></package>'
    )
    cover = epub_xhtml(
        '<section epub:type="cover"><img src="cover.png" alt="Cover"/></section>'
    )
    return epub_zip(
        [
            ("OEBPS/content.opf", opf),
            ("OEBPS/cover.xhtml", cover),
            ("OEBPS/cover.png", comic_png(1, 1, (200, 0, 0))),
            ("OEBPS/nav.xhtml", epub_xhtml(nav)),
            ("OEBPS/text/ch1.xhtml", epub_xhtml(ch1)),
            ("OEBPS/text/ch 2.xhtml", epub_xhtml(ch2)),
        ],
        "OEBPS/content.opf",
    )


def epub2_opf(items: str, spine: str, title: str = "Second Book") -> str:
    # The DOCTYPE older package files carry.
    return (
        '<?xml version="1.0"?>\n'
        '<!DOCTYPE package PUBLIC "+//ISBN 0-9673008-1-9//DTD OEB 1.2 Package//EN" '
        '"http://openebook.org/dtds/oeb-1.2/oebpk12.dtd">\n'
        '<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">'
        '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">'
        f'<dc:identifier id="id">urn:uuid:2</dc:identifier><dc:title>{title}</dc:title>'
        "<dc:creator>Cy Penn</dc:creator></metadata>"
        f"<manifest>{items}</manifest>"
        f'<spine toc="ncx">{spine}</spine></package>'
    )


def epub_book2() -> bytes:
    xhtml = "application/xhtml+xml"
    items = (
        '<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>'
        f'<item id="one" href="one.html" media-type="{xhtml}"/>'
        f'<item id="notes" href="notes.html" media-type="{xhtml}"/>'
        f'<item id="broken" href="broken.html" media-type="{xhtml}"/>'
        f'<item id="two" href="two.html" media-type="{xhtml}"/>'
    )
    spine = (
        '<itemref idref="one"/><itemref idref="notes" linear="no"/>'
        '<itemref idref="ghost"/><itemref idref="broken"/><itemref idref="two"/>'
    )
    ncx = (
        '<?xml version="1.0"?>\n'
        '<!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" '
        '"http://www.daisy.org/z3986/2005/ncx-2005-1.dtd">\n'
        '<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><navMap>'
        '<navPoint id="n1" playOrder="1"><navLabel><text>One</text></navLabel>'
        '<content src="one.html"/>'
        '<navPoint id="n2" playOrder="2"><navLabel><text>Notes</text></navLabel>'
        '<content src="notes.html#n1"/></navPoint></navPoint></navMap></ncx>'
    )
    one = (
        "\ufeff"
        + epub_xhtml(
            "<p>Hello&nbsp;world.</p>\n  \n"
            '<p>Note<a id="r1"/><sup><a href="notes.html#n1">1</a></sup> here.</p>\n'
            '<a id="end"/>'
        )
    ).encode()
    notes = epub_xhtml('<p id="n1">The note. <a href="one.html#end">Back</a></p>')
    broken = "<html><body><p>unclosed</body></html>"
    two = epub_xhtml("<p>Caf\u00e9</p>").encode("latin-1")
    return epub_zip(
        [
            ("content.opf", epub2_opf(items, spine)),
            ("toc.ncx", ncx),
            ("one.html", one),
            ("notes.html", notes),
            ("broken.html", broken),
            ("two.html", two),
        ],
        "content.opf",
    )


def epub_encrypted(encrypted: str, font: bool) -> bytes:
    xhtml = "application/xhtml+xml"
    items = f'<item id="c" href="c.xhtml" media-type="{xhtml}"/>'
    if font:
        items += '<item id="f" href="fonts/f.otf" media-type="font/otf"/>'
    files: list[tuple[str, bytes | str]] = [
        ("content.opf", epub2_opf(items, '<itemref idref="c"/>', "Locked")),
        ("c.xhtml", epub_xhtml("<p>Text.</p>")),
        (
            "META-INF/encryption.xml",
            '<?xml version="1.0"?>'
            '<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" '
            'xmlns:enc="http://www.w3.org/2001/04/xmlenc#">'
            '<enc:EncryptedData><enc:EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/>'
            f'<enc:CipherData><enc:CipherReference URI="{encrypted}"/></enc:CipherData>'
            "</enc:EncryptedData></encryption>",
        ),
    ]
    if font:
        files.append(("fonts/f.otf", b"OTTO\x00\x01\x02\x03"))
    return epub_zip(files, "content.opf")


def sample_md() -> str:
    """Markdown using every construct the block tree knows."""
    lines = [
        "---",
        "title: Sample",
        "---",
        "",
        "# Café *naïve* heading",
        "",
        "Setext heading",
        "==============",
        "",
        'Plain **strong** and ~~struck~~ text, a `code span`, a [link](http://example.org "Title"),',
        'an image ![alt text](pic.png), an escaped \\*star\\*, an entity &amp; "quotes" -- a dash.',
        "A hard break  ",  # two trailing spaces
        "and another\\",
        "line.<br> A footnote[^note].",
        "",
        "- [x] done",
        "- [ ] todo",
        "",
        "3. three",
        "4. four",
        "",
        "> Quoted",
        "> > nested",
        "",
        "```rust",
        "fn main() {}",
        "```",
        "",
        "    indented code",
        "",
        "| Left | Centre |",
        "|:-----|:------:|",
        "| a    | b      |",
        "",
        "***",
        "",
        "<div>html</div>",
        "",
        "[ref]: http://example.org",
        "",
        "[^note]: The note.",
    ]
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    (Path(__file__).resolve().parent / "sample.md").write_bytes(sample_md().encode())
    here = Path(__file__).resolve().parent
    (here / "outline.pdf").write_bytes(outline())
    (here / "outline-empty.pdf").write_bytes(outline_empty())
    (here / "outline-deep.pdf").write_bytes(outline_deep())
    (here / "page-labels.pdf").write_bytes(page_labels())
    (here / "two-pages.pdf").write_bytes(two_pages())
    (here / "text.pdf").write_bytes(text())
    (here / "tj.pdf").write_bytes(tj())
    (here / "links.pdf").write_bytes(links())
    (here / "properties.pdf").write_bytes(properties())
    (here / "black.pdf").write_bytes(black())
    (here / "mixed-sizes.pdf").write_bytes(mixed_sizes())
    (here / "tiling.pdf").write_bytes(tiling())
    (here / "type3.pdf").write_bytes(type3())
    (here / "shared-names.pdf").write_bytes(shared_names())
    (here / "comic.cbz").write_bytes(comic())
    (here / "comic-deflated.cbz").write_bytes(comic(zipfile.ZIP_DEFLATED))
    (here / "comic-alpha.cbz").write_bytes(comic_alpha())
    (here / "comic-broken.cbz").write_bytes(comic_broken())
    (here / "comic-damaged.cbz").write_bytes(comic_damaged())
    (here / "comic-duplicate.cbz").write_bytes(comic_duplicate())
    (here / "comic-empty.cbz").write_bytes(comic_empty())
    (here / "comic-huge.cbz").write_bytes(comic_huge())
    (here / "comic-bad-info.cbz").write_bytes(comic_bad_info())
    (here / "book.epub").write_bytes(epub_book())
    (here / "book2.epub").write_bytes(epub_book2())
    (here / "drm.epub").write_bytes(epub_encrypted("c.xhtml", False))
    (here / "obfuscated.epub").write_bytes(epub_encrypted("fonts/f.otf", True))
