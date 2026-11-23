# md-editor-core

Headless engine. Everything here must run in a test without a display or GPU.

- **No UI dependencies.** Never add `iced`, `winit`, `ratex-render` or other GUI/shaping
  crates.
- **Atomic writes only.** Every vault write goes through `vault/files.rs::write_file`
  (sibling temp file, permission copy, `sync_all`, rename, parent-dir sync). Never truncate
  a user file in place.
- **Paths are jailed.** Resolve user-supplied vault paths with `resolve_vault_path_checked`;
  vault-relative strings always use `/`.
- **Vault switches are generational.** Work that outlives a switch (indexing, graph
  snapshots) captures `current_vault_generation()` and the root under
  `vault_publish_lock`, and drops its result if the generation moved. Do not hold these
  locks across disk or PDFium work.
- **PDFs and decks are read-only.** Annotations live in `pdf_annotations` /
  `pdf_references`, keyed by the vault-scoped `document_id`. One `Pdfium` binding per
  process, reached only through `pdf/`.
- **Schema changes** go in `state.rs` as a new `user_version` migration; never edit an old
  one.
- **Reference resolver output changes** must bump `references::RESOLVER_VERSION`.
- Wikilink parsing/resolution changes must pass `massive_tests.rs`.

Deeper context: `wiki/Core-Services.md`, `wiki/PDF-Engine-and-Sidecars.md`,
`wiki/Data-Flows-and-Durability-Invariants.md`.
