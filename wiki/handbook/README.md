# The Field Handbook — build sources

This directory generates **`../MD-Editor-Handbook.pdf`**, a ~100-page printable book that
teaches the repository to a new contributor. It is written from the wiki pages one level up
and verified against the source tree; where a wiki page and the code disagreed, the code won.

## Rebuilding

```bash
pip install playwright pypdf     # once
python3 build.py                 # writes ../MD-Editor-Handbook.pdf
```

The build needs a Chromium binary. `build.py` prefers one already on the machine
(`$PLAYWRIGHT_BROWSERS_PATH`, then `chromium` / `google-chrome` on `PATH`); if none is
found, `playwright install chromium` provides one.

## How it works

1. `part*.html` are concatenated in numeric order into `build/handbook.html`.
2. Headless Chromium renders it twice — page 1 (the cover) without a footer, pages 2+ with
   a running footer — and the two are merged, so page numbering stays consistent while the
   cover stays clean.
3. Each chapter carries an invisible marker span (`class="pm"`, text `PMK-<id>-`). The
   script extracts per-page text with `pypdf` to find which page each marker landed on.
4. The contents list is filled in with those page numbers and the document is re-rendered.
5. A PDF outline (bookmarks) and document metadata are attached.

## Editing

| File | Contents |
| :--- | :--- |
| `part00-head.html` | the print stylesheet — page setup, type scale, callouts, figure and SVG helper classes |
| `part01-cover.html` | cover and “How to use this handbook” |
| `part02-toc.html` | the contents list; page numbers are placeholders filled at build time |
| `part03`–`part04` | Part I — Orientation (chapters 1–4) |
| `part05`–`part06` | Part II — The engine (chapters 5–10) |
| `part07`–`part09` | Part III — The markdown editor (chapters 11–17) |
| `part10`–`part12` | Part IV — The PDF subsystem (chapters 18–25) |
| `part13` | Part V — The surface (chapters 26–28) |
| `part14`–`part15` | Part VI — Working here (chapters 29–33) |
| `part16`–`part17` | Appendices A–E |

Ordering is by the numeric prefix, so a new file needs a number, not a rename of everything
after it. Files are plain HTML fragments — no templating, no build step beyond `build.py`.

### Adding a chapter

1. Give the `<section>` an `id`, and put `<span class="pm">PMK-&lt;id&gt;-</span>` inside its
   chapter number so the page-number pass can find it.
2. Add a row to the contents list in `part02-toc.html` with a matching
   `<span class="pg" data-ref="&lt;id&gt;">00</span>`.
3. Add an entry to `OUTLINE` in `build.py` so it appears in the PDF bookmarks.

### Figures

Every diagram is hand-authored inline SVG — there is no diagram library and no generated
graph. Conventions used throughout:

- `viewBox="0 0 720 H"`; 720 units is the text column, so a figure never overflows.
- Text uses the `.t`, `.t-sm`, `.t-xs`, `.t-mono-sm` classes from `part00-head.html`;
  boxes use `.s-box`, `.s-fill`, `.s-fill2`, `.s-warn`, `.s-amber`.
- SVG collapses runs of whitespace, so **columns are aligned with explicit `x` positions**,
  not with padding spaces.
- Wrap each one in `<figure>` with a `<figcaption>` that states the claim the picture makes,
  and give the `<svg>` a `role="img"` and an `aria-label` carrying the same claim.

Every claim in the book should be checkable against the file named beside it. If the book
and the code disagree, the code is right and the book has a bug.
