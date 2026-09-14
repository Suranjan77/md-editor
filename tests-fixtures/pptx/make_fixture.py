"""Build `fixture.pptx`, the presentation the PPTX parser tests read.

The decks the viewer was first written against come from a generator that
places every shape absolutely and colours it explicitly. Real PowerPoint files
lean on inheritance instead, so this fixture starts from python-pptx's default
template (a genuine PowerPoint master, layouts and theme) and exercises what
those decks never do: placeholders that take their position and text style
from the layout and master, theme colours, style references, groups with a
child transform, connectors, rotation, merged table cells, a chart, fields,
numbered lists, and gradient and picture fills.

Regenerate with:

    python -m venv .venv && .venv/bin/pip install python-pptx==1.0.2
    .venv/bin/python tests-fixtures/pptx/make_fixture.py

The parser tests pin facts about this exact file, so regenerate it only
together with those tests.
"""

import copy
import io
import pathlib
import struct
import zlib

from lxml import etree
from pptx import Presentation
from pptx.chart.data import CategoryChartData
from pptx.dml.color import RGBColor
from pptx.enum.chart import XL_CHART_TYPE
from pptx.enum.dml import MSO_THEME_COLOR
from pptx.enum.shapes import MSO_CONNECTOR, MSO_SHAPE
from pptx.enum.text import MSO_ANCHOR, PP_ALIGN
from pptx.oxml.ns import qn
from pptx.util import Inches, Pt

OUT = pathlib.Path(__file__).with_name("fixture.pptx")


def solid_png(width, height, rgb):
    """A tiny single-colour PNG, so the fixture needs no image files."""
    row = b"\x00" + bytes(rgb) * width
    raw = row * height

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def add_xml(parent, markup):
    """Append raw DrawingML to `parent`; python-pptx has no API for some of it."""
    element = etree.fromstring(
        '<root xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">'
        + markup
        + "</root>"
    )[0]
    parent.append(element)
    return element


prs = Presentation()

# Slide 1 — title layout. Both placeholders carry no geometry of their own.
slide = prs.slides.add_slide(prs.slide_layouts[0])
slide.shapes.title.text = "Fixture Deck"
slide.placeholders[1].text = "Placeholders inherit from the layout"

# Slide 2 — title and content: bullet levels and mixed runs.
slide = prs.slides.add_slide(prs.slide_layouts[1])
slide.shapes.title.text = "Bullets"
frame = slide.placeholders[1].text_frame
frame.text = "First level"
para = frame.add_paragraph()
para.text = "Second level"
para.level = 1
para = frame.add_paragraph()
run = para.add_run()
run.text = "Mixed "
run = para.add_run()
run.text = "bold"
run.font.bold = True
run = para.add_run()
run.text = " and red"
run.font.color.rgb = RGBColor(0xC0, 0x00, 0x00)

# Slide 3 — blank layout: styled shapes, rotation, a scaled group, a
# connector and a picture over a solid background.
slide = prs.slides.add_slide(prs.slide_layouts[6])
background = slide.background.fill
background.solid()
background.fore_color.rgb = RGBColor(0xF5, 0xF2, 0xEC)

styled = slide.shapes.add_shape(
    MSO_SHAPE.ROUNDED_RECTANGLE, Inches(0.5), Inches(0.5), Inches(3), Inches(1.5)
)
styled.text = "Styled shape"

oval = slide.shapes.add_shape(MSO_SHAPE.OVAL, Inches(4), Inches(0.5), Inches(2), Inches(1.5))
oval.fill.solid()
oval.fill.fore_color.theme_color = MSO_THEME_COLOR.ACCENT_2
oval.rotation = 30

group = slide.shapes.add_group_shape()
group.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(1), Inches(3), Inches(1), Inches(1))
group.shapes.add_shape(
    MSO_SHAPE.ISOSCELES_TRIANGLE, Inches(2.5), Inches(3), Inches(1), Inches(1)
)
# python-pptx leaves a group's child space equal to its own extent. Halve the
# extent so the children are drawn at half size from the group's origin.
group_xfrm = group._element.grpSpPr.find(qn("a:xfrm"))
ext = group_xfrm.find(qn("a:ext"))
ext.set("cx", str(int(ext.get("cx")) // 2))
ext.set("cy", str(int(ext.get("cy")) // 2))

connector = slide.shapes.add_connector(
    MSO_CONNECTOR.STRAIGHT, Inches(4), Inches(3), Inches(7), Inches(4)
)
connector.line.width = Pt(3)
connector.line.color.rgb = RGBColor(0x2C, 0x6E, 0x72)

slide.shapes.add_picture(
    io.BytesIO(solid_png(4, 2, (0x6B, 0x4E, 0x7A))),
    Inches(7),
    Inches(5),
    Inches(2),
    Inches(1),
)

# Slide 4 — title only: a table with merged cells, and a chart.
slide = prs.slides.add_slide(prs.slide_layouts[5])
slide.shapes.title.text = "Table and chart"
table = slide.shapes.add_table(3, 3, Inches(0.5), Inches(1.5), Inches(5), Inches(1.5)).table
for r in range(3):
    for c in range(3):
        table.cell(r, c).text = f"R{r}C{c}"
table.cell(0, 1).merge(table.cell(0, 2))
table.cell(1, 0).merge(table.cell(2, 0))

chart_data = CategoryChartData()
chart_data.categories = ["a", "b"]
chart_data.add_series("Series", (1, 2))
slide.shapes.add_chart(
    XL_CHART_TYPE.COLUMN_CLUSTERED,
    Inches(6),
    Inches(1.5),
    Inches(3.5),
    Inches(3),
    chart_data,
)

# Slide 5 — text layout: a gradient background, alignment, anchoring, a
# line break, a numbered list, a superscript, and a slide-number field.
slide = prs.slides.add_slide(prs.slide_layouts[6])
background = slide.background.fill
background.gradient()
background.gradient_angle = 90
background.gradient_stops[0].color.rgb = RGBColor(0xFF, 0xFF, 0xFF)
background.gradient_stops[1].color.rgb = RGBColor(0xDD, 0xE6, 0xF0)

box = slide.shapes.add_textbox(Inches(0.5), Inches(0.5), Inches(4), Inches(2))
frame = box.text_frame
frame.word_wrap = True
frame.vertical_anchor = MSO_ANCHOR.BOTTOM
frame.text = "Centered first line\vafter a break"
frame.paragraphs[0].alignment = PP_ALIGN.CENTER
for label in ["One", "Two", "Three"]:
    para = frame.add_paragraph()
    para.text = label
    para.font.size = Pt(14)
    ppr = para._p.get_or_add_pPr()
    ppr.set("marL", "342900")
    ppr.set("indent", "-342900")
    add_xml(ppr, '<a:buAutoNum type="arabicPeriod"/>')

box = slide.shapes.add_textbox(Inches(5), Inches(0.5), Inches(4), Inches(1))
para = box.text_frame.paragraphs[0]
para.alignment = PP_ALIGN.RIGHT
run = para.add_run()
run.text = "E = mc"
run = para.add_run()
run.text = "2"
run.font._rPr.set("baseline", "30000")
run = para.add_run()
run.text = " — slide "
field = add_xml(
    para._p,
    '<a:fld id="{B6F15528-21DE-4FAA-801E-634DDDAF4B2B}" type="slidenum">'
    '<a:rPr lang="en-US"/><a:t>‹#›</a:t></a:fld>',
)
# `add_xml` appends after `a:endParaRPr`, which must stay last.
end = para._p.find(qn("a:endParaRPr"))
if end is not None:
    para._p.remove(end)
    para._p.append(end)

prs.save(OUT)
print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")
