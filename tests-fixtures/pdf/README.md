# PDF fixture corpus

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
