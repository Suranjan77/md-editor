# Architecture Rules

Referenced by `AGENTS.md`. Machine-enforced by `scripts/architecture-check.sh`
(run in CI by `.github/workflows/quality.yml`). If a rule here and the script
disagree, fix the script — the rule wins.

## Crate graph

```
native (iced shell)  ──►  core (pure logic + persistence)
```

- `core` must not depend on `native`, `iced`, or `winit`. *(enforced)*
- `native` must not use `rusqlite` or `pdfium_render` directly — persistence and PDF
  access go through core services. *(enforced)*

## Inside `core`

```
application ──► domain + infrastructure + database
infrastructure ──► domain
database ──► domain
domain ──► (nothing)
```

- `domain/` holds value types only (ids, paths, pdf geometry, annotations, notes,
  sessions). No I/O, no services.
- `application/` holds the services that are core's public API:
  `VaultService`, `SearchService`, `PdfService`, `TrackerService` (+ planned
  `AnnotationService`, `LinkGraphService`).
- `infrastructure/` holds adapters: pdfium, fs, config store, indexer.
- `database/` holds SQLite repositories, private to core (repositories are not
  exported; services own them). Its fate vs `infrastructure/` is an open ADR
  (see plan Appendix A).
- Legacy flat files (`vault.rs`, `state.rs`, `file_index.rs`, `tracker.rs`,
  `types.rs`) are dissolving into the layout above during Phase 2; they may only
  shrink (see `budgets.toml`).

## Inside `native`

- `features/<name>/` owns each feature's messages, state, update, view.
  **Features never import each other** — cross-feature signals route through the
  root reducer (`AppEvent` pattern, Phase 3). *(enforced; the three pre-existing
  violations are allowlisted in `architecture-check.sh` and that list may only
  shrink)*
- `views/` is transitional; Phase 3 reclassifies it into `widgets/` (reusable)
  and per-feature view modules. `widgets/` must never import `features/`.
- `editor/` is a library: buffer, parser, renderer. The renderer never gains
  parsing rules *(enforced)*; layout/draw stays viewport-bounded — no
  full-document scans on hot paths.
- `design/` (UX-A) holds tokens/palette/styles and imports nothing from
  `features/`.

## State & persistence

- `AppState` infrastructure fields stay private; persistence goes through
  database repositories. *(enforced)*
- All Markdown buffer mutations go through `EditorCommand`; `set_text` only for
  file load / whole-buffer transactions. *(enforced)*

## Budgets (ratchets)

`budgets.toml` freezes current file sizes, unwrap count, and raw-color count as
ceilings. `scripts/check-budget.sh` and `scripts/unwrap-budget.sh` fail CI if a
number rises. When you reduce one, lower the ceiling in the same PR.

## Verifying a rule fires

The enforcement scripts were proven by injecting violations (e.g. `use iced::Color;`
into core, a cross-feature import into shell) and observing CI-failure exit codes;
do the same when you add a rule.
