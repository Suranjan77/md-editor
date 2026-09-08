#!/usr/bin/env python3
"""
Build MD-Editor-Handbook.pdf from the HTML sources in this directory.

Pipeline
--------
1.  Concatenate part*.html (sorted) into build/handbook.html.
2.  Render it with headless Chromium (Playwright) — twice:
      * page 1 (the cover) with no footer,
      * pages 2..N with a running footer carrying the page number.
    The two renders are merged, so page numbering stays globally consistent
    while the cover stays clean.
3.  Locate every chapter by the invisible marker span it carries (class="pm",
    text "PMK-<id>-"), extracting per-page text with pypdf.
4.  Substitute the real page numbers into the contents list and re-render.
5.  Attach a PDF outline (bookmarks) and document metadata.

Requires: playwright (+ the Chromium at $PLAYWRIGHT_BROWSERS_PATH), pypdf.
"""

from __future__ import annotations

import io
import re
import shutil
import sys
from pathlib import Path

from playwright.sync_api import sync_playwright
from pypdf import PdfReader, PdfWriter

def _chromium() -> str | None:
    """Prefer the Chromium already on the machine over Playwright's pinned revision."""
    import os

    root = Path(os.environ.get("PLAYWRIGHT_BROWSERS_PATH", "/opt/pw-browsers"))
    for pattern in ("chromium-*/chrome-linux/chrome",
                    "chromium_headless_shell-*/chrome-linux/headless_shell"):
        hits = sorted(root.glob(pattern))
        if hits:
            return str(hits[-1])
    return shutil.which("chromium") or shutil.which("google-chrome")


HERE = Path(__file__).resolve().parent
BUILD = HERE / "build"
OUT = HERE.parent / "MD-Editor-Handbook.pdf"

TITLE = "MD Editor — The Field Handbook"

FOOTER = """
<div style="width:100%;font-family:'Liberation Sans',Arial,sans-serif;font-size:7.5pt;
            color:#727b7e;padding:0 17mm;display:flex;justify-content:space-between;
            border-top:.5pt solid #d5dcda;padding-top:2mm;margin-top:4mm;">
  <span>MD Editor — The Field Handbook</span>
  <span class="pageNumber"></span>
</div>"""

EMPTY = "<div></div>"

# (outline label, marker id, nesting level)
OUTLINE: list[tuple[str, str, int]] = [
    ("Part I — Orientation", "ch01", 0),
    ("1. What you are looking at", "ch01", 1),
    ("2. The map: two crates, three kinds of thread", "ch02", 1),
    ("3. Your first hour", "ch03", 1),
    ("4. The language of this codebase", "ch04", 1),
    ("Part II — The engine (md-editor-core)", "ch05", 0),
    ("5. AppState and the lock discipline", "ch05", 1),
    ("6. The database", "ch06", 1),
    ("7. Where the database lives", "ch07", 1),
    ("8. The atomic save", "ch08", 1),
    ("9. Wikilinks and the backlink graph", "ch09", 1),
    ("10. Full-text search", "ch10", 1),
    ("Part III — The markdown editor", "ch11", 0),
    ("11. The shell: one loop, one router", "ch11", 1),
    ("12. Subscriptions and the zero-idle-CPU rule", "ch12", 1),
    ("13. DocBuffer, transactions, and word-sized undo", "ch13", 1),
    ("14. Hybrid preview: conceal and reveal", "ch14", 1),
    ("15. The Fenwick height tree", "ch15", 1),
    ("16. Measure once: the line cache", "ch16", 1),
    ("17. The draw pass", "ch17", 1),
    ("Part IV — The PDF subsystem", "ch18", 0),
    ("18. One thread, two queues", "ch18", 1),
    ("19. Two coordinate systems", "ch19", 1),
    ("20. Finding a table of contents that isn't there", "ch20", 1),
    ("21. Resolving \"see equation (3.14)\"", "ch21", 1),
    ("22. Sidecar annotations and document identity", "ch22", 1),
    ("23. Generations, pending sets, and the stranded page", "ch23", 1),
    ("24. Stable page slots", "ch24", 1),
    ("25. From highlight to note and back", "ch25", 1),
    ("Part V — The surface", "ch26", 0),
    ("26. Tokens and motion", "ch26", 1),
    ("27. The command palette and the fuzzy scorer", "ch27", 1),
    ("28. The study tracker", "ch28", 1),
    ("Part VI — Working here", "ch29", 0),
    ("29. The build system and PDFium", "ch29", 1),
    ("30. The test suite, and what each part protects", "ch30", 1),
    ("31. The five durability invariants", "ch31", 1),
    ("32. The rules, and the bug behind each one", "ch32", 1),
    ("33. Shipping a build", "ch33", 1),
    ("Appendix A — Keyboard shortcuts", "apA", 0),
    ("Appendix B — Every named constant", "apB", 0),
    ("Appendix C — The message vocabulary", "apC", 0),
    ("Appendix D — Starter tasks, graded", "apD", 0),
    ("Appendix E — Where to go next", "apE", 0),
]


def assemble() -> str:
    parts = sorted(HERE.glob("part*.html"),
                   key=lambda p: int(re.match(r"part(\d+)", p.name).group(1)))
    if not parts:
        sys.exit("no part*.html files found")
    html = "\n".join(p.read_text(encoding="utf-8") for p in parts)
    if "</body>" not in html:
        html += "\n</body>\n</html>\n"
    return html


def render(page, html_path: Path) -> bytes:
    """Render page 1 without a footer and the rest with one, then merge."""
    page.goto(html_path.as_uri(), wait_until="networkidle")
    page.emulate_media(media="print")

    common = dict(
        print_background=True,
        prefer_css_page_size=True,
        display_header_footer=True,
        header_template=EMPTY,
    )
    cover = page.pdf(page_ranges="1", footer_template=EMPTY, **common)
    body = page.pdf(page_ranges="2-", footer_template=FOOTER, **common)

    writer = PdfWriter()
    for blob in (cover, body):
        for pg in PdfReader(io.BytesIO(blob)).pages:
            writer.add_page(pg)
    buf = io.BytesIO()
    writer.write(buf)
    return buf.getvalue()


def marker_pages(pdf_bytes: bytes) -> dict[str, int]:
    """Map marker id -> 1-based page number, from the invisible PMK-<id>- tokens."""
    reader = PdfReader(io.BytesIO(pdf_bytes))
    found: dict[str, int] = {}
    for n, pg in enumerate(reader.pages, start=1):
        try:
            text = pg.extract_text() or ""
        except Exception:
            text = ""
        for mid in re.findall(r"PMK-([A-Za-z0-9]+)-", text):
            found.setdefault(mid, n)
    return found


def fill_contents(html: str, pages: dict[str, int]) -> str:
    def sub(m: re.Match[str]) -> str:
        ref = m.group(1)
        return f'<span class="pg" data-ref="{ref}">{pages.get(ref, "—")}</span>'

    return re.sub(r'<span class="pg" data-ref="([A-Za-z0-9]+)">[^<]*</span>', sub, html)


def add_outline(pdf_bytes: bytes, pages: dict[str, int]) -> bytes:
    reader = PdfReader(io.BytesIO(pdf_bytes))
    writer = PdfWriter()
    for pg in reader.pages:
        writer.add_page(pg)

    parent = None
    for label, ref, level in OUTLINE:
        idx = pages.get(ref)
        if idx is None:
            continue
        if level == 0:
            parent = writer.add_outline_item(label, idx - 1)
        else:
            writer.add_outline_item(label, idx - 1, parent=parent)

    writer.add_metadata(
        {
            "/Title": TITLE,
            "/Author": "MD Editor project",
            "/Subject": "A guided tour of the md-editor codebase for new contributors",
            "/Keywords": "Rust, Iced, PDFium, SQLite, markdown editor, onboarding",
            "/Creator": "wiki/handbook/build.py",
        }
    )
    writer.page_layout = "/SinglePage"
    writer.page_mode = "/UseOutlines"

    buf = io.BytesIO()
    writer.write(buf)
    return buf.getvalue()


def main() -> None:
    if BUILD.exists():
        shutil.rmtree(BUILD)
    BUILD.mkdir(parents=True)

    html = assemble()
    src = BUILD / "handbook.html"

    with sync_playwright() as pw:
        browser = pw.chromium.launch(executable_path=_chromium())
        page = browser.new_page()

        # pass 1 — placeholders, just to learn where everything landed
        src.write_text(html, encoding="utf-8")
        first = render(page, src)
        pages = marker_pages(first)
        print(f"pass 1: {len(PdfReader(io.BytesIO(first)).pages)} pages, "
              f"{len(pages)}/{len({r for _, r, _ in OUTLINE})} markers located")

        missing = sorted({r for _, r, _ in OUTLINE} - pages.keys())
        if missing:
            print(f"  WARNING: markers not found: {', '.join(missing)}")

        # pass 2 — contents filled in
        src.write_text(fill_contents(html, pages), encoding="utf-8")
        final = render(page, src)
        after = marker_pages(final)
        browser.close()

    drift = {k: (pages[k], after[k]) for k in pages if after.get(k) != pages[k]}
    if drift:
        print(f"  note: pagination shifted for {len(drift)} marker(s) after filling "
              f"the contents; using the second pass for the outline.")
    pages = {**pages, **after}

    OUT.write_bytes(add_outline(final, pages))
    n = len(PdfReader(io.BytesIO(final)).pages)
    print(f"wrote {OUT.relative_to(HERE.parent.parent)} — {n} pages, "
          f"{OUT.stat().st_size / 1024:.0f} KB")


if __name__ == "__main__":
    main()
