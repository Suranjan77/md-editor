#!/usr/bin/env python3
"""Deterministic PDF fixture generator (stdlib only).

Regenerates the corpus in tests-fixtures/pdf/. Each fixture is a minimal but
spec-valid PDF built from raw bytes so output is byte-identical across runs.

Usage:
    python3 scripts/gen-fixtures.py            # small committed fixtures
    python3 scripts/gen-fixtures.py --large    # also the 500-page CI fixture
"""

import argparse
import os
import sys

OUT_DIR = os.path.join(os.path.dirname(__file__), "..", "tests-fixtures", "pdf")


class PdfBuilder:
    """Assembles a PDF from numbered objects, computing the xref table."""

    def __init__(self):
        self.objects = {}  # obj number -> bytes (body without "N 0 obj"/"endobj")

    def add(self, num, body):
        if isinstance(body, str):
            body = body.encode("latin-1")
        self.objects[num] = body

    def stream(self, num, content, extra_dict=""):
        if isinstance(content, str):
            content = content.encode("latin-1")
        body = b"<< /Length %d %s >>\nstream\n%s\nendstream" % (
            len(content),
            extra_dict.encode("latin-1"),
            content,
        )
        self.objects[num] = body

    def build(self, root_num):
        out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
        offsets = {}
        for num in sorted(self.objects):
            offsets[num] = len(out)
            out += b"%d 0 obj\n" % num
            out += self.objects[num]
            out += b"\nendobj\n"
        xref_pos = len(out)
        count = max(self.objects) + 1
        out += b"xref\n0 %d\n" % count
        out += b"0000000000 65535 f \n"
        for num in range(1, count):
            if num in offsets:
                out += b"%010d 00000 n \n" % offsets[num]
            else:
                out += b"0000000000 65535 f \n"
        out += b"trailer\n<< /Size %d /Root %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
            count,
            root_num,
            xref_pos,
        )
        return bytes(out)


def text_stream(lines, font="F1", size=12, x=72, y=720, leading=16):
    ops = ["BT", "/%s %d Tf" % (font, size), "%d %d Td" % (x, y), "%d TL" % leading]
    for i, line in enumerate(lines):
        if i > 0:
            ops.append("T*")
        escaped = line.replace("\\", r"\\").replace("(", r"\(").replace(")", r"\)")
        ops.append("(%s) Tj" % escaped)
    ops.append("ET")
    return "\n".join(ops)


def helvetica(num):
    return "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"


def simple_doc(page_texts, rotate=None, annots=None, outline=None, page_extra=None):
    """Build a text PDF. page_texts: list of line-lists, one per page."""
    b = PdfBuilder()
    n_pages = len(page_texts)
    # Layout: 1 catalog, 2 pages-root, 3 font, then per page: page obj, content obj.
    font_num = 3
    first_page = 4
    page_nums = [first_page + 2 * i for i in range(n_pages)]
    kids = " ".join("%d 0 R" % p for p in page_nums)
    catalog = "<< /Type /Catalog /Pages 2 0 R"
    next_free = first_page + 2 * n_pages
    if outline:
        outline_root = next_free
        catalog += " /Outlines %d 0 R" % outline_root
    catalog += " >>"
    b.add(1, catalog)
    b.add(2, "<< /Type /Pages /Kids [%s] /Count %d >>" % (kids, n_pages))
    b.add(font_num, helvetica(font_num))
    for i, lines in enumerate(page_texts):
        page_num = page_nums[i]
        content_num = page_num + 1
        page = (
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            "/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R" % (font_num, content_num)
        )
        if rotate and i in rotate:
            page += " /Rotate %d" % rotate[i]
        if annots and i in annots:
            page += " /Annots [%s]" % annots[i](page_nums)
        if page_extra and i in page_extra:
            page += " " + page_extra[i]
        page += " >>"
        b.add(page_num, page)
        b.stream(content_num, text_stream(lines))
    if outline:
        outline(b, next_free, page_nums)
    return b.build(1)


def fixture_single_page():
    return simple_doc([[
        "Single-page fixture",
        "The quick brown fox jumps over the lazy dog.",
        "Used for: open, render, text extraction smoke tests.",
    ]])


def fixture_multipage_outline():
    texts = [
        ["Chapter %d" % (i + 1), "Body text for chapter %d." % (i + 1)] for i in range(5)
    ]

    def outline(b, root, page_nums):
        # root, then one item per chapter
        items = [root + 1 + i for i in range(5)]
        b.add(root, "<< /Type /Outlines /First %d 0 R /Last %d 0 R /Count 5 >>" % (items[0], items[-1]))
        for i, item in enumerate(items):
            entry = "<< /Title (Chapter %d) /Parent %d 0 R /Dest [%d 0 R /XYZ 0 792 null]" % (
                i + 1, root, page_nums[i])
            if i > 0:
                entry += " /Prev %d 0 R" % items[i - 1]
            if i < 4:
                entry += " /Next %d 0 R" % items[i + 1]
            entry += " >>"
            b.add(item, entry)

    return simple_doc(texts, outline=outline)


def fixture_internal_links():
    texts = [
        ["Page 1: the words below link to page 2.", "CLICK HERE"],
        ["Page 2: link target."],
    ]

    def annots_p1(page_nums):
        return (
            "<< /Type /Annot /Subtype /Link /Rect [72 690 200 715] /Border [0 0 0] "
            "/Dest [%d 0 R /XYZ 0 792 null] >>" % page_nums[1]
        )

    # annot must be its own object normally, but inline dict in /Annots array is legal.
    return simple_doc(texts, annots={0: annots_p1})


def fixture_rotated():
    return simple_doc(
        [["Rotated 0"], ["Rotated 90"], ["Rotated 180"], ["Rotated 270"]],
        rotate={1: 90, 2: 180, 3: 270},
    )


def fixture_cjk():
    """CJK text via a CIDFont with Identity-H encoding referencing a standard font.

    pdfium extracts the unicode via the ToUnicode CMap even without an embedded
    font program, which is exactly what text-extraction tests need.
    """
    b = PdfBuilder()
    b.add(1, "<< /Type /Catalog /Pages 2 0 R >>")
    b.add(2, "<< /Type /Pages /Kids [4 0 R] /Count 1 >>")
    # Type0 font: 3=Type0, 6=CIDFont, 7=ToUnicode CMap
    b.add(
        3,
        "<< /Type /Font /Subtype /Type0 /BaseFont /MS-Gothic /Encoding /Identity-H "
        "/DescendantFonts [6 0 R] /ToUnicode 7 0 R >>",
    )
    b.add(
        4,
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        "/Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>",
    )
    # Text: 日本語 (U+65E5 U+672C U+8A9E) written as CIDs 1,2,3
    content = "BT\n/F1 24 Tf\n72 700 Td\n<000100020003> Tj\nET"
    b.stream(5, content)
    b.add(
        6,
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /MS-Gothic "
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
        "/FontDescriptor 8 0 R /DW 1000 /CIDToGIDMap /Identity >>",
    )
    cmap = (
        "/CIDInit /ProcSet findresource begin\n"
        "12 dict begin\nbegincmap\n"
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
        "/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n"
        "1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n"
        "3 beginbfchar\n<0001> <65E5>\n<0002> <672C>\n<0003> <8A9E>\nendbfchar\n"
        "endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend"
    )
    b.stream(7, cmap)
    b.add(
        8,
        "<< /Type /FontDescriptor /FontName /MS-Gothic /Flags 5 "
        "/FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 800 /Descent -200 "
        "/CapHeight 700 /StemV 80 >>",
    )
    return b.build(1)


def fixture_tight_leading():
    b = PdfBuilder()
    b.add(1, "<< /Type /Catalog /Pages 2 0 R >>")
    b.add(2, "<< /Type /Pages /Kids [4 0 R] /Count 1 >>")
    b.add(3, helvetica(3))
    b.add(
        4,
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        "/Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>",
    )
    lines = [
        "Tight-leading line 1",
        "Tight-leading line 2",
        "Tight-leading line 3",
        "Tight-leading line 4",
        "Tight-leading line 5",
        "Tight-leading line 6",
    ]
    b.stream(5, text_stream(lines, size=12, leading=12))
    return b.build(1)


def fixture_two_column():
    b = PdfBuilder()
    b.add(1, "<< /Type /Catalog /Pages 2 0 R >>")
    b.add(2, "<< /Type /Pages /Kids [4 0 R] /Count 1 >>")
    b.add(3, helvetica(3))
    b.add(
        4,
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        "/Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>",
    )
    col1 = text_stream([
        "Col 1 Line 1",
        "Col 1 Line 2",
        "Col 1 Line 3",
        "Col 1 Line 4",
    ], x=72, y=700)
    col2 = text_stream([
        "Col 2 Line 1",
        "Col 2 Line 2",
        "Col 2 Line 3",
        "Col 2 Line 4",
    ], x=320, y=700)
    b.stream(5, col1 + "\n" + col2)
    return b.build(1)


def fixture_corrupt():
    """Valid header, then garbage and a truncated xref: must error, not panic."""
    good = fixture_single_page()
    return good[: len(good) // 3] + b"\x00\xff GARBAGE NOT A PDF \xde\xad\xbe\xef"


def fixture_500_pages():
    texts = [["Page %d of 500" % (i + 1), "Stress fixture for virtualized rendering."]
             for i in range(500)]
    return simple_doc(texts)


FIXTURES = {
    "single-page.pdf": fixture_single_page,
    "multipage-outline.pdf": fixture_multipage_outline,
    "internal-links.pdf": fixture_internal_links,
    "rotated-pages.pdf": fixture_rotated,
    "cjk-text.pdf": fixture_cjk,
    "tight-leading.pdf": fixture_tight_leading,
    "two-column.pdf": fixture_two_column,
    "corrupt.pdf": fixture_corrupt,
}


LARGE_FIXTURES = {
    "large-500-pages.pdf": fixture_500_pages,
}


README = """# PDF fixture corpus

Generated deterministically by `python3 scripts/gen-fixtures.py` (stdlib only —
do not edit these files by hand; edit the generator).

| File | Purpose |
|---|---|
| single-page.pdf | Smallest valid doc: open/render/extract smoke tests. |
| multipage-outline.pdf | 5 pages with a bookmark tree (one entry per chapter): outline/TOC tests. |
| internal-links.pdf | Page 1 has a /Link annotation jumping to page 2: link hit-testing & navigation. |
| rotated-pages.pdf | Pages with /Rotate 0/90/180/270: geometry & rendering orientation. |
| cjk-text.pdf | Identity-H CID font with ToUnicode CMap spelling 日本語: unicode text extraction. |
| tight-leading.pdf | Single page with overlap-prone 12pt font and 12pt leading: text selection tests. |
| two-column.pdf | Single page with side-by-side text columns: multi-column text selection tests. |
| corrupt.pdf | Truncated/garbage tail: loaders must return an error, never panic. |
| large-500-pages.pdf | (Not committed; `--large`, generated in CI.) Virtualized-rendering stress doc. |

All content is generated, license-free, and < 200 KB per committed file.
"""


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--large", action="store_true", help="also generate the 500-page fixture")
    args = parser.parse_args()

    os.makedirs(OUT_DIR, exist_ok=True)
    todo = dict(FIXTURES)
    if args.large:
        todo.update(LARGE_FIXTURES)
    for name, fn in sorted(todo.items()):
        data = fn()
        path = os.path.join(OUT_DIR, name)
        with open(path, "wb") as f:
            f.write(data)
        print("wrote %s (%d bytes)" % (os.path.relpath(path), len(data)))
    with open(os.path.join(OUT_DIR, "README.md"), "w") as f:
        f.write(README)
    print("wrote README.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
