# Coding Standards

This repository uses one Rust style for every contributor and agent.

## Required Commands

Run before publishing changes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

If `just` is installed, use:

```bash
just check
```

`cargo clippy` runs with `-D warnings`. A small set of legacy style lints is
allowed at crate root so CI can start blocking new warnings immediately. Remove
those allowances gradually when touching nearby code.

## Formatting

- Use repo `rustfmt.toml`.
- Keep formatting-only changes in dedicated commits or PRs.
- Do not run broad formatting when making a narrow feature unless the resulting
  changes are part of the feature files.

## Naming

Use consistent verbs:

- `open_*`: load a user-visible resource and change active view.
- `load_*`: fetch or cache data without changing route.
- `render_*`: create visual assets.
- `navigate_*`: change page, scroll, cursor, or focus target.
- `sync_*`: reconcile derived state from source state.
- `build_*`: pure value construction.
- `default_*`: deterministic fallback value.

Use explicit nouns for ambiguous values:

- `page_index`: 0-based PDF page index.
- `page_number`: 1-based user-visible page number.
- `vault_path`: path relative to vault root.
- `abs_path`: absolute filesystem path.
- `annotation_id`: PDF annotation id.

## Error Handling

- Avoid `unwrap()` and `expect()` outside tests.
- Do not silently discard user-action failures.
- Background/cache failures may be nonfatal, but must be recorded or surfaced
  when they explain missing UI output.
- Use `let _ = ...` only for best-effort side effects where failure is harmless
  and documented by nearby context.

## Editor Rules

- All document mutations go through `EditorCommand`, except initial file load.
- Every mutating editor command needs undo/redo coverage.
- Parser output must preserve source: concatenating `StyledSpan.text` for a
  physical line should reconstruct that source line unless the line is a
  synthetic placeholder.
- Parsing lives in `native/src/editor/highlight.rs`.
- Rendering lives in `native/src/editor/renderer.rs`.
- Renderer must not add markdown parsing rules.

## PDF Rules

- Store PDF page indexes as 0-based values internally.
- Convert to 1-based page numbers only for display and markdown links.
- Keep PDF annotations in sidecar storage; never modify source PDFs.
- `pdf://` links are navigation targets and must remain stable.

## State And Cache Rules

- Keep derived/cache fields close to invalidation logic.
- Prefer named reset/invalidation helpers over scattered manual field clears.
- Layout and draw work must be proportional to visible content.
- Add a regression test before fixing a bug when the behavior is reproducible.
