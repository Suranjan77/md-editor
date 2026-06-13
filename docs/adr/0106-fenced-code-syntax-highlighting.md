# ADR-0106: Fenced-code syntax highlighting stays outside the renderer

- Status: accepted (2026-06-13)
- Scope: v3 Markdown fenced-code highlighting.
- Spec: `V3_IMPLEMENTATION_PLAN.md` Phase 7.6.

## Context

Phase 7.3 renders fenced code as a measured monospace block, but every code
span currently has one `Code` paint role. Adding language rules directly to
`v3/shell/src/gui/paint.rs` would make rendering parse source text, duplicate
the editor's incremental parsing ownership, and put potentially unbounded
work on the viewport draw path.

The incremental Markdown parser already identifies fence boundaries and the
optional language tag. Syntax highlighting must preserve that ownership
boundary and the shaped-layout rule that measure, paint, and hit-testing use
one geometry source.

## Decision

- `v3/editor` owns language-aware tokenization for fenced-code content.
  Markdown fence detection and language extraction remain in `parse.rs`.
- Syntax tokens are cached document state keyed by line revision and fence
  language. Editing invalidates the changed line and any following stateful
  lexer lines until lexer state converges.
- Tokens expose semantic roles, not theme colors. Initial roles are comment,
  keyword, string, number, type, function, operator, and punctuation.
- `v3/shell` maps semantic roles to theme colors while building the existing
  draw plan. Renderer code never detects languages or applies syntax rules.
- Highlighting changes paint only. Token boundaries do not alter font,
  shaping, wrapping, line height, caret geometry, selection geometry, or
  source/display offset mapping.
- Unknown and empty language tags fall back to the existing single-color
  code role. Highlighting failure is non-fatal and uses the same fallback.
- Tokenization is incremental and outside the viewport paint hot path.
  Initial document load may schedule background work; stale results are
  discarded by document revision.
- Library selection and implementation remain a separate Phase 7.6 slice.
  Any dependency must support stateful line-by-line tokenization without
  embedding a second Markdown parser.

## Verification Contract

- Golden draw-plan test covers known-language tokens and unknown-language
  fallback.
- Editing inside a multiline construct verifies invalidation continues until
  lexer state converges.
- Geometry before and after token installation is identical.
- A stale async result cannot overwrite tokens for a newer document revision.
- Budget test proves paint visits only visible lines and performs no syntax
  parsing.

## Consequences

- Phase 7.3's code-block geometry remains stable while syntax color arrives.
- Themes can change without re-tokenizing.
- Parser and renderer ownership stay explicit.
- Syntax highlighting is still unimplemented; this ADR removes design
  ambiguity but does not mark that refinement complete.
