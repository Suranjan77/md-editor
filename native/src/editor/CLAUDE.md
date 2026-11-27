# Markdown editor

`buffer/` (text, caret, undo) → `highlight.rs` (styled lines, incremental) →
`renderer/` (the `iced` widget). Full design: `wiki/Markdown-Pipeline.md`; rules with their
reasons: `wiki/Contributor-Guidelines.md` Rule 3.

- **Draw only what is visible.** The draw pass iterates `visible_start..visible_end` from
  the `HeightTree`, never the whole document.
- **Layout is cached.** A frame whose `LayoutKey` matches skips layout. New state that
  affects a line's height must feed `line_hash`/`resource_hash` *and* change
  `EditorPane::layout_revision` or `LayoutKey`.
- **One wrapper.** Inline text is measured and broken into rows only in `renderer/flow.rs`;
  caret, painting, highlights and hit testing read the `Flow`.
- **Shared sizes live in `renderer/metrics.rs`.** Vertical sizes sit on the 4px `GRID`;
  gaps go through `requested_margins`.
- **Caret state lives in `DocBuffer`.** The widget keeps no copy.
- **A new block type** updates block ranges, height measurement, draw metadata and hit
  testing together.
- **Clip with layers** (`renderer.with_layer`), not the clip rect of `fill_text`/`draw_image`.
- **Highlighting is synchronous and incremental.** `highlight/tests.rs` checks incremental
  results equal a fresh parse and the oracle in `highlight/reference.rs`; change both on a
  deliberate behaviour change.

Tests: renderer bugs get a test in `renderer/tests.rs` (against draw calls and messages,
seen failing first); general rules go in `renderer/properties.rs` — run
`RENDER_PROPERTY_CASES=3000 cargo test -p md-editor-native properties` before landing.
