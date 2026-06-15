# Md editor — project notes for agents

This project is a **dark-only markdown/PDF study-vault app** ("Quiet Vault" redesign).

## Design system
**Always read `docs/DESIGN-SYSTEM.md` before designing any surface, and match the reference implementation `docs/quiet-vault-reference.html` exactly.** Do not invent colors, fonts, or chrome.

## Non-negotiables (summary — full detail in the doc)
- **Dark only.** Canvas `#0e0e12`, rails `#0a0a0d`, chrome `#0c0c10`, surfaces `#16161c`/`#101014`. Hairline borders `rgba(255,255,255,0.05–0.07)`.
- **Accent:** purple `#bd93f9` (sparingly). **Wikilinks:** teal `#67c6b0`. No other accents.
- **Type:** `Hanken Grotesk` (UI + body), `Geist Mono` (code/numerals). Never Inter/Roboto/Arial.
- **Headings by weight/size, not color.**
- **Line icons only** (16-viewBox, stroke currentColor). **No emoji. No decorative gradients.**
- **No menu bar** — global actions go through the command spine (⌘K) and palette.
- **Document-first:** the note is a centered ~750px sheet; file tree + study tracker are quiet, collapsible rails.
- **Study tracker is a ledger** (daily log) + projects/gates/reading tracker — not a live stopwatch.
- **Layout:** every scrolling flex child needs `min-height:0` up its chain or it won't scroll.
- Overlays (palette, search, dialogs) all follow the command-palette archetype in the doc.

## Migration
The phased plan to move the current UI to this design lives in `docs/QUIET-VAULT-MIGRATION.md`.
