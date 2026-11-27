# Page override — Markdown editor (document typography)

Overrides MASTER §3 and §2 for content *inside* a note. Layout rhythm stays in
`native/src/editor/renderer/metrics.rs`, which already follows a 4px grid; this page changes
measure, colour roles and the reveal style, not the layout engine.

## Measure and rhythm

| Token | Today | Proposed | Why |
| :--- | :--- | :--- | :--- |
| Body | Inter 17 / 1.6 | keep | Already within the skill's 1.5–1.75 line-height |
| `MAX_CONTENT_WIDTH` | 880px (~95–100ch) | **720px default (~72ch)**; Settings: Narrow 640 / Default 720 / Wide 880 | Skill `line-length`: 65–75 characters |
| Heading scale | literals per level | ×1.25 modular scale from 17: 21 · 27 · 33 (H3 · H2 · H1); H4–H6 at 17 with weight and spacing | One scale instead of per-level literals |
| Code | 15px system mono | JetBrains Mono 15 / 1.5 | Bundled, tabular |
| Font size literals in `highlight/*.rs` (17, 16, 14, 13) | — | Replaced by roles from `metrics.rs` (`BODY`, `CODE`, `MATH_SOURCE`, `CAPTION`) | Single source |

## Document colour roles (both themes)

| Role | Dark | Light | Replaces |
| :--- | :--- | :--- | :--- |
| `doc.text` | `fg.primary` | `fg.primary` | `TEXT_PRIMARY` |
| `doc.syntax` (revealed `**`, `#`, `[]()`) | `fg.tertiary` | `fg.tertiary` | `TEXT_MUTED` |
| `doc.link` | `accent` + underline on hover | `accent` | `ACCENT` |
| `doc.code.fg` / `doc.code.bg` | `fg.secondary` / `bg.raised` | same roles | ad-hoc |
| `doc.math` | `fg.primary` (rendered), `fg.secondary` source | same | **`WARNING`** (was misused) |
| `doc.quote.bar` | `border.control` | same | `BORDER` |
| `doc.selection` | `accent` @ 28% | `accent` @ 20% | — |
| `doc.find` / `doc.find.active` | `highlight` @ 30% / `highlight` @ 55% + 1px `highlight` outline | same | distinct from selection by outline, not only hue |
| `doc.caret` | `accent` | `accent` | `ACCENT_SECONDARY` |
| `doc.block.bg` (code, table, math cards) | `bg.surface` on `bg.canvas` | `bg.raised` on `bg.surface` | — |
| Syntax highlighting theme | syntect theme generated from tokens; for now syntect's bundled `base16-ocean.dark` | for now `InspiredGitHub` (`highlight/code.rs::code_highlighter`) | one fixed theme |

## Reveal ("edit-in-place") styling

Revealed Markdown markers (`**`, `$…$`, `#`) render in `doc.syntax` at the same size as the
text, and revealed TeX keeps the upright math source style, so revealing a line reads as
"the same line, now editable". Today a revealed line switches to italic source and a
row-wide band; check against a current build before changing it.

## States

Empty document: placeholder line "Start writing…" in `fg.tertiary`. Read-only/failed-to-open:
inline banner above the page with the reason and a Retry action.
