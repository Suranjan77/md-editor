# Codebase Restructuring Plan

## Purpose

Restructure repository without changing user-visible behavior. Primary goals:

- Reduce cognitive load in high-change modules.
- Make ownership and dependency direction explicit.
- Isolate pure domain logic from Iced, SQLite, filesystem, and PDFium adapters.
- Keep editor and PDF hot paths bounded by visible content.
- Make feature work testable without constructing full `MdEditor`.
- Preserve continuous release capability during migration.

This is an incremental refactor plan, not a rewrite. Each phase should produce
small reviewable pull requests with passing quality gates.

## Current Baseline

Snapshot from June 7, 2026:

| Area | Current size | Main concern |
| --- | ---: | --- |
| `native/src/app.rs` | 10,001 lines | 108 fields, update routing, orchestration, storage calls, task creation, rendering composition, and tests |
| `native/src/editor/renderer.rs` | 4,801 lines | Measurement, layout, drawing, events, hit testing, movement, scrollbars, and tests |
| `native/src/editor/highlight.rs` | 2,322 lines | Block parsing, inline parsing, metadata extraction, syntax highlighting, and tests |
| `native/src/editor/buffer.rs` | 2,180 lines | Text model, commands, transactions, movement, formatting, tables, and tests |
| `core/src/pdf.rs` | 1,749 lines | PDF domain types, worker protocol, PDFium adapter, scheduling, rendering, extraction, search, and tests |
| `core/src/vault.rs` | 1,554 lines | File operations, path resolution, indexing, backlinks, unified search, PDF text search, and tests |
| `core/src/state.rs` | 796 lines | Dependency container, schema creation, migrations, PDF repositories, and config path logic |
| `native/src/messages.rs` | 328 lines | Roughly 214 flat application message variants |
| `native/src/main.rs` | 434 lines | Runtime entry point plus Linux desktop integration |

Strengths already present:

- Workspace split between UI and core.
- `EditorCommand` transaction model.
- Parser/renderer ownership rule.
- PDF generation checks and bounded page scheduling.
- Height tree and layout cache.
- Broad unit and UI fixture coverage.
- CI format, Clippy, and workspace test gates.

Main structural risks:

- `MdEditor` acts as state store, reducer, controller, service locator, and view
  model.
- Flat `Message` enum couples unrelated features and forces one giant update
  match.
- Native layer reaches into SQLite directly.
- `AppState` exposes mutexes and concrete infrastructure publicly.
- Startup duplicates database schema setup between disk and in-memory paths.
- Renderer methods share broad widget state and many implicit invariants.
- Tests cluster inside production files, making moves noisy and files harder to
  scan.
- Crate-level lint allowances hide ownership and API design problems.
- Native depends directly on `rusqlite`, weakening core boundary.

## Non-Goals

- No UI redesign.
- No editor engine replacement.
- No Iced framework replacement.
- No async runtime replacement.
- No database format reset.
- No source PDF mutation.
- No broad formatting-only churn mixed with moves.
- No speculative microservices, plugin system, or generic event bus.
- No trait abstraction where only one implementation exists and no test seam is
  needed.

## Design Principles

### Dependency Rule

Dependencies point inward:

```text
views -> feature coordinators -> application services -> domain -> adapters
  |              |                       |               |
 Iced          Message/Task           use cases       SQLite/PDFium/fs
```

Practical Rust mapping:

- `domain`: data and pure rules; no Iced, SQLite, filesystem, or PDFium.
- `application`: use cases and orchestration over narrow ports.
- `infrastructure`: SQLite, filesystem, PDFium, OS integration.
- `presentation`: Iced state, messages, views, and task mapping.

Do not force four crates immediately. Establish module boundaries first. Split
crates only after dependency edges become stable.

### Preferred Patterns

Use patterns where they remove current complexity:

- **Command**: retain `EditorCommand` for all document mutations.
- **Reducer**: feature state plus feature message produces state change and
  effects.
- **Facade**: expose narrow `VaultService`, `SearchService`, and `PdfService`
  APIs over infrastructure.
- **Repository**: isolate settings, tracker, PDF documents, annotations, and
  search persistence.
- **Adapter**: wrap PDFium, SQLite, filesystem, and desktop integration.
- **State machine**: model PDF loading/search/render generations and tracker
  running state explicitly.
- **Strategy**: search ranking/source selection and PDF render scheduling where
  policies vary.
- **Newtype**: prevent path and page-index unit confusion.
- **Builder/input object**: replace large view/function argument lists.
- **Cache-as-component**: state, key, invalidation, and metrics live together.
- **Strangler migration**: route one feature at a time out of `app.rs`.

Avoid patterns likely to add ceremony:

- Global service locator.
- Generic observer/event bus.
- Deep trait hierarchies.
- One trait per concrete type.
- Macro-generated reducers before message boundaries stabilize.
- ECS-style state decomposition.

## Target Repository Shape

Target is directional. Exact names may adjust during extraction.

```text
core/src/
  lib.rs
  domain/
    mod.rs
    path.rs
    search.rs
    tracker.rs
    pdf/
      mod.rs
      annotation.rs
      geometry.rs
      link.rs
      text.rs
  application/
    mod.rs
    vault_service.rs
    search_service.rs
    tracker_service.rs
    pdf_service.rs
    integrity_service.rs
  infrastructure/
    mod.rs
    database/
      mod.rs
      connection.rs
      migrations.rs
      settings_repository.rs
      tracker_repository.rs
      pdf_repository.rs
      search_repository.rs
    filesystem/
      mod.rs
      vault_repository.rs
      path_resolver.rs
      reference_repair.rs
    pdfium/
      mod.rs
      binding.rs
      worker.rs
      renderer.rs
      text.rs
      links.rs
  file_index.rs

native/src/
  main.rs
  platform/
    mod.rs
    desktop_integration.rs
  app/
    mod.rs
    model.rs
    update.rs
    view.rs
    subscription.rs
    startup.rs
    effects.rs
  features/
    shell/
    workspace/
    editor/
    pdf/
    search/
    citations/
    tracker/
    overlays/
  editor/
    buffer/
      mod.rs
      command.rs
      transaction.rs
      movement.rs
      formatting.rs
      table.rs
    parser/
      mod.rs
      model.rs
      block.rs
      inline.rs
      metadata.rs
      syntax.rs
    renderer/
      mod.rs
      widget.rs
      measure.rs
      draw.rs
      hit_test.rs
      movement.rs
      scrollbar.rs
      geometry.rs
      state.rs
    layout/
      mod.rs
      tree.rs
      cache.rs
  views/
    ...
```

Keep parser implementation physically under `native/src/editor`, satisfying
existing parser ownership rule. Rename `highlight` to `parser` only after
callers stop depending on internal parsing details.

## Target Application Model

Replace 108-field flat `MdEditor` state with cohesive feature state:

```rust
pub struct MdEditor {
    services: Services,
    shell: ShellState,
    workspace: WorkspaceState,
    editor: EditorFeatureState,
    pdf: PdfFeatureState,
    search: SearchFeatureState,
    citations: CitationState,
    tracker: TrackerState,
    overlays: OverlayState,
}
```

Rules:

- Feature state owns its derived values and invalidation helpers.
- Shared source of truth stays singular. Do not clone active paths into several
  feature states.
- Cross-feature actions flow through app coordinator.
- Views receive immutable input structs, not full `MdEditor`.
- Feature reducers never execute filesystem, database, or PDFium work directly.
  They return `Task<Message>` or typed effect requests.

Message hierarchy:

```rust
pub enum Message {
    Shell(ShellMessage),
    Workspace(WorkspaceMessage),
    Editor(EditorMessage),
    Pdf(PdfMessage),
    Search(SearchMessage),
    Citation(CitationMessage),
    Tracker(TrackerMessage),
    Overlay(OverlayMessage),
    System(SystemMessage),
}
```

Top-level update becomes routing plus cross-feature coordination:

```rust
match message {
    Message::Editor(message) => self.editor.update(message, &context),
    Message::Pdf(message) => self.pdf.update(message, &context),
    // Cross-feature outcomes handled through explicit app effects.
}
```

Do not move all variants at once. Introduce nested messages feature by feature.

## Workstream 1: Baseline And Guardrails

Goal: make structural progress measurable and detect accidental behavior drift.

Tasks:

1. Record baseline:
   - Test count and duration.
   - Debug and release build duration.
   - Startup time with empty and representative vaults.
   - Editor frame time for 1,000-line and 10,000-line documents.
   - PDF scroll frame time for 100-page document.
   - Search latency for representative vault.
   - Peak PDF page-cache bytes.
2. Add architecture decision records under `docs/adr/`:
   - Dependency direction.
   - Feature reducer/message model.
   - Repository boundaries.
   - Path/page newtypes.
3. Add `cargo metadata`/dependency checks to CI.
4. Add simple architecture checks:
   - `core` must not depend on `native`.
   - `views` must not import SQLite or PDFium.
   - renderer must not add parser dependencies.
   - native application code must not call `rusqlite` after repository phase.
5. Track file-size budgets as warning metrics, not immediate hard failures.
6. Add representative smoke fixtures for markdown, PDF, split view, search,
   annotations, and tracker.

Acceptance:

- Existing quality commands pass.
- Benchmarks or timing harnesses produce repeatable local output.
- Refactor PR template links relevant ADR and risk.

## Workstream 2: Core Infrastructure Boundary

Goal: stop exposing database mutex and raw SQL to native.

Current problem:

- `AppState` publicly exposes `db`, `vault_root`, `file_index`, `pdf_state`, and
  concrete renderer.
- `native/src/app.rs` and `native/src/integrity.rs` issue SQL.
- Schema setup is duplicated in `new()` and `new_in_memory()`.

Plan:

1. Extract `Database::open(path)` and `Database::open_in_memory()`.
2. Centralize schema creation and migrations in
   `infrastructure/database/migrations.rs`.
3. Introduce concrete repositories first:
   - `SettingsRepository`
   - `TrackerRepository`
   - `PdfDocumentRepository`
   - `PdfAnnotationRepository`
   - `SearchRepository`
4. Move native SQL into core repository methods.
5. Make mutex fields private.
6. Replace `AppState` with `CoreServices` or keep name temporarily while
   exposing methods only.
7. Return typed error enums from core boundaries. Convert to user-facing strings
   in native.
8. Remove native `rusqlite` dependency after final direct use disappears.

Repository guidance:

- Use concrete structs for production.
- Add traits only where in-memory/fake implementations materially simplify
  application tests.
- Keep transaction boundaries in application service methods, not UI code.
- Keep SQL row mapping beside repository query.

Acceptance:

- `rg "rusqlite" native/src` returns no production hits.
- Disk and memory database initialization share one schema path.
- No public database mutex.
- Migration tests cover fresh database and upgrade path.

## Workstream 3: Domain Types And Unit Safety

Goal: encode common invariants in types.

Introduce:

```rust
pub struct VaultPath(PathBuf);
pub struct AbsPath(PathBuf);
pub struct PageIndex(u16);
pub struct PageNumber(NonZeroU16);
pub struct DocumentId(String);
pub struct AnnotationId(String);
pub struct RenderGeneration(u64);
pub struct SearchGeneration(u64);
```

Migration rules:

- Newtypes start at core/native service boundaries.
- Avoid mass conversion in renderer hot loops until profiling confirms no cost.
- `PageIndex` remains internal; convert to `PageNumber` only for display/link
  construction.
- `VaultPath` must reject absolute paths and traversal outside vault.
- `AbsPath` construction resolves or validates filesystem semantics explicitly.
- IDs expose borrowed string views to avoid repeated allocation.

Also split `core/src/types.rs` by domain. Keep re-exports during migration to
reduce caller churn.

Acceptance:

- New service APIs cannot accept ambiguous `&str` paths or pages.
- PDF link tests prove 0-based/1-based conversion.
- Vault traversal and separator normalization tests pass cross-platform.

## Workstream 4: App State Decomposition

Goal: reduce `MdEditor` to composition root and coordinator.

Extraction order minimizes cross-feature coupling:

1. `OverlayState`
   - modal
   - command palette
   - citation palette visibility/query
   - toast
2. `TrackerState`
   - visibility/running/timing
   - sessions/config/manual-entry fields
3. `ShellState`
   - sidebar/backlinks/TOC visibility
   - split ratios
   - active panel
   - window and keyboard state
4. `SearchFeatureState`
   - editor search
   - global search
   - PDF search status/generations
5. `WorkspaceState`
   - vault root and entries
   - selected/active resources
   - expanded folders
   - navigation history
6. `EditorFeatureState`
   - buffer
   - highlighted projection
   - highlight scheduling
   - editor viewport
   - media caches
7. `PdfFeatureState`
   - document identity
   - layout/page cache
   - render/search/text generations
   - selection/annotations
   - TOC/links/scroll

For each extraction:

1. Move fields into state struct without moving behavior.
2. Add named reset and invalidation methods.
3. Move pure selectors.
4. Move message variants into nested enum.
5. Move update arms.
6. Move tests.
7. Remove compatibility forwarding methods after callers migrate.

State methods should describe invariants:

- `pdf.begin_document_load(...)`
- `pdf.begin_render_generation(...)`
- `pdf.complete_page_render(...)`
- `pdf.invalidate_text_cache()`
- `editor.schedule_highlight(...)`
- `search.begin_global_search(...)`
- `workspace.activate_resource(...)`
- `overlays.close_topmost()`

Avoid public field mutation across features.

Acceptance:

- Top-level `MdEditor` has fewer than 12 feature-level fields.
- Top-level `update` stays below roughly 300 lines.
- Feature tests construct feature state without full app fixture.
- Reset behavior exists in named methods, not repeated field-clearing blocks.

## Workstream 5: Effects And Async Task Discipline

Goal: make side effects and stale-result handling explicit.

Introduce typed effect/output enums where cross-feature coordination exists:

```rust
pub enum PdfEffect {
    None,
    Task(Task<PdfMessage>),
    OpenLinkedNote(VaultPath),
    InsertEditorCommand(EditorCommand),
    ShowToast(String),
}
```

Alternative: return `Task<Message>` directly for local effects and typed
`AppEffect` only for cross-feature actions. Prefer minimum abstraction.

Rules:

- Every async request carries typed generation or request ID.
- Completion clears pending state before generation rejection.
- Cancellation is explicit for searchable/streamed work.
- Effects own error conversion and user-facing context.
- Feature reducers remain deterministic except task construction.
- Long-running CPU or blocking work stays outside Iced update thread.

State-machine candidates:

- PDF document lifecycle:
  `Empty -> Loading -> Ready -> Failed`.
- PDF search:
  `Idle -> Searching(id) -> Complete | Failed | Cancelled`.
- Highlight:
  `Current -> Debouncing(generation) -> Running(generation) -> Current`.
- Tracker:
  `Stopped -> Running(started_at) -> Stopped`.

Do not create one universal state-machine framework. Use enums and exhaustive
matches per feature.

Acceptance:

- Stale-result tests exist beside each async feature.
- Pending sets cannot leak entries after stale completion.
- Feature update tests assert state transition and emitted task/effect.

## Workstream 6: Editor Buffer Decomposition

Goal: preserve command model while separating editing concerns.

Suggested split:

- `command.rs`: `EditorCommand`, `Movement`, command classification.
- `transaction.rs`: `EditOp`, `EditTransaction`, undo/redo.
- `movement.rs`: grapheme and line movement.
- `formatting.rs`: inline/block formatting.
- `table.rs`: table bounds and row/column operations.
- `document.rs`: `DocBuffer`, source text, cursor, selection.

Migration:

1. Move types with no behavior change.
2. Group private methods by concern using internal modules.
3. Keep `DocBuffer::execute` as command dispatcher.
4. Make mutation primitives private to buffer module.
5. Replace legacy convenience mutation methods if unused.
6. Add command metadata helpers:
   - `changes_text()`
   - `changes_projection()`
   - `may_change_media()`
   - `should_keep_cursor_visible()`
7. Move `editor_command_keeps_cursor_visible` from app into command metadata.

Testing:

- Table-driven tests for every command.
- Undo/redo round trip property for mutating commands.
- Cursor/selection invariants after random command sequences.
- Unicode grapheme regression suite.
- Source text unchanged for navigation-only commands.

Acceptance:

- All mutations still enter through `EditorCommand`.
- No `buffer.set_text` outside load/whole-buffer transaction and tests.
- `DocBuffer::execute` remains clear command routing, not feature logic.
- Buffer module has no Iced dependency.

## Workstream 7: Parser Decomposition

Goal: keep parsing centralized while making phases readable.

Suggested phases:

1. Normalize physical lines without changing source.
2. Detect block context.
3. Parse inline spans.
4. Apply syntax highlighting for fenced code.
5. Produce document metadata.

Suggested modules:

- `model.rs`: `StyledLine`, `StyledSpan`, block/span flags.
- `block.rs`: headings, lists, quotes, fences, tables, math blocks.
- `inline.rs`: emphasis, links, images, code, math, escapes.
- `syntax.rs`: Syntect integration.
- `metadata.rs`: outline, links, anchors, frontmatter.
- `reference.rs`: reference definitions and resolution.

Rules:

- Source reconstruction remains mandatory.
- Renderer consumes parser output only.
- Metadata extraction reuses parser tokens; no second regex parser in app/core.
- Parsing APIs accept source and return immutable projection.
- Parser errors degrade to source-preserving plain spans.

Potential optimization:

- Introduce incremental line/block parsing only after module split and benchmark
  baseline. Do not combine architecture move with algorithm replacement.

Acceptance:

- Concatenated `StyledSpan.text` reconstructs every physical source line.
- Existing markdown feature matrix remains covered.
- Parser public surface is small and documented.
- No markdown grammar logic appears in renderer.

## Workstream 8: Renderer Decomposition

Goal: separate pure geometry from Iced widget plumbing without harming hot paths.

Suggested split:

- `widget.rs`: `Editor` builder and `Widget` implementation.
- `state.rs`: persistent widget state and cache ownership.
- `measure.rs`: line/span dimensions and wrapping.
- `draw.rs`: visible range and painting.
- `hit_test.rs`: point-to-line/column/block mapping.
- `movement.rs`: visual cursor navigation.
- `scrollbar.rs`: block horizontal-scroll geometry and interaction.
- `geometry.rs`: pure rectangles, ranges, clipping, coordinate conversion.

Extraction order:

1. Pure geometry functions.
2. Horizontal scrollbar calculations.
3. Hit-test result types and pure helpers.
4. Measurement context/input structs.
5. Drawing context/input structs.
6. Visual movement.
7. Thin final `Widget` implementation.

Use input objects to replace long argument lists:

```rust
struct MeasureContext<'a, R> {
    renderer: &'a R,
    width: f32,
    focused: bool,
    active_block_id: Option<usize>,
    resources: &'a EditorResources,
}
```

Performance constraints:

- No full-document scan in layout/draw/update hot paths.
- Preserve height-tree `O(log N)` lookup.
- Visible block metadata remains viewport bounded.
- Context structs borrow data; avoid cloning lines, spans, images, or maps.
- Pure helper extraction must compile to equivalent loops.
- Benchmark before and after each large renderer move.

Acceptance:

- `renderer/mod.rs` explains pipeline and exports narrow widget API.
- Draw module cannot mutate document.
- Geometry and hit testing have direct unit tests.
- Large-document and viewport performance does not regress beyond agreed noise,
  suggested 5%.

## Workstream 9: PDF Core And Native Feature Split

Goal: separate PDF domain, PDFium infrastructure, worker scheduling, and UI
coordination.

Core split:

- `domain/pdf/geometry.rs`: `PdfRect`, merge logic.
- `domain/pdf/annotation.rs`: annotation types and validation.
- `domain/pdf/text.rs`: text page/line/character types.
- `domain/pdf/link.rs`: link and preview result types.
- `infrastructure/pdfium/binding.rs`: library discovery/binding.
- `infrastructure/pdfium/document.rs`: cached document access.
- `infrastructure/pdfium/render.rs`: page and preview rendering.
- `infrastructure/pdfium/text.rs`: extraction and search.
- `infrastructure/pdfium/worker.rs`: commands, channels, priority scheduling,
  cancellation.
- `application/pdf_service.rs`: stable API used by native.

Native split:

- `features/pdf/state.rs`
- `features/pdf/message.rs`
- `features/pdf/update.rs`
- `features/pdf/tasks.rs`
- `features/pdf/navigation.rs`
- `features/pdf/annotations.rs`
- `features/pdf/search.rs`
- `features/pdf/view_model.rs`

Consolidate existing `pdf_layout`, `pdf_page_cache`, `pdf_navigation`,
`pdf_links`, and `pdf_notes` under coherent feature/domain ownership. Do not
merge pure helpers back into update module.

Important invariants:

- PDFium calls remain process-wide serialized unless proven safe by supported
  library version.
- Worker owns PDF document handles.
- Page indexes remain 0-based.
- Generation cleanup occurs before stale-result return.
- Cache invalidation stays beside cache ownership.
- Rendering and overlays remain viewport bounded.
- Annotations remain sidecar data.

Acceptance:

- Native app coordinator does not know PDFium command protocol.
- PDF feature can reset document state through one method.
- Render scheduling policy has isolated deterministic tests.
- Existing PDF architecture document updates to new paths.

## Workstream 10: Vault, Indexing, And Search

Goal: separate filesystem operations from indexing and query composition.

Current `vault.rs` responsibilities should split into:

- `VaultRepository`: list/read/write/create/rename/delete.
- `VaultPathResolver`: safe vault-relative resolution.
- `ReferenceRepairService`: rename reference updates.
- `MarkdownIndexService`: parsed metadata to search/backlink index.
- `VaultSearchService`: markdown/file result queries.
- `PdfTextSearchRepository`: cached PDF text queries.
- `UnifiedSearchService`: source selection, ranking, caps, merge.

Search design:

- Query object remains typed.
- Source adapters return normalized candidates.
- Ranking policy is pure and testable.
- Result caps occur at explicit layer.
- Preview generation is separate pure formatter.
- Cancellation/request IDs stay native/application concern.

Index design:

- Parser supplies markdown links and metadata.
- Core does not invent parallel markdown regex grammar.
- Save and rename use explicit transaction/use-case flow:

```text
validate path
-> write/rename file
-> repair references when needed
-> update file index
-> update FTS/backlinks
-> return changed paths
```

Define failure semantics for partial operations before changing implementation.
Where atomic filesystem/database transaction is impossible, add recovery and
reindex path.

Acceptance:

- `vault.rs` becomes facade or disappears.
- Path safety tests cover traversal, separators, root rename, and Unicode.
- Unified ranking tests require no filesystem or database.
- Reindex can rebuild derived data from vault source files.

## Workstream 11: Views And Presentation Inputs

Goal: keep views declarative and prevent business logic leakage.

Rules:

- View functions consume purpose-built immutable inputs.
- Views emit feature messages.
- Views do not query database/filesystem.
- Views do not mutate caches.
- View modules may format labels but not decide domain policy.
- Complex enablement rules live in selectors or command registry.

Introduce:

```rust
pub struct PdfToolbarViewModel { ... }
pub struct SearchPanelViewModel { ... }
pub struct StatusBarViewModel { ... }
```

Prefer borrowed fields where lifetime ergonomics remain reasonable. Use owned
small display strings when that substantially simplifies Iced element lifetime
handling.

Consolidate:

- Shared focusable row/button behavior.
- Repeated empty/loading/error panel patterns.
- Repeated modal shell.
- Common labeled icon button variants.

Do not build generic design-system machinery until at least three concrete uses
exist.

Acceptance:

- No view takes `&MdEditor`.
- View files contain composition and message wiring only.
- UI fixture tests cover each view model state.

## Workstream 12: Platform And Startup

Goal: keep executable entry point small and OS-specific code isolated.

Move Linux desktop install/uninstall logic from `main.rs` to:

```text
native/src/platform/desktop_integration.rs
```

Separate:

- CLI argument parsing.
- Desktop integration.
- Application startup.
- Service construction.
- Iced runtime launch.

Use a `StartupOptions` value. Return structured errors from platform operations
instead of booleans.

Acceptance:

- `main.rs` contains argument parse, service construction, and launch only.
- Platform modules compile behind target `cfg`.
- Install/uninstall tests cover generated desktop entry without touching real
  home directory.

## Testing Reorganization

### Test Layers

1. Pure unit tests:
   - paths
   - page conversions
   - parser
   - buffer commands
   - geometry
   - ranking
   - state transitions
2. Component tests:
   - repositories against in-memory SQLite
   - filesystem repository against temp vault
   - PDF worker against fixture PDF
   - feature reducer plus fake service
3. Integration tests:
   - save/index/search
   - rename/reference repair
   - PDF open/render/search/annotation
   - cross-pane navigation
4. UI fixture tests:
   - shell modes
   - overlays
   - command availability
   - accessibility labels
5. Performance tests:
   - parser throughput
   - renderer visible-range work
   - PDF scheduling/cache
   - unified search

### Test Placement

- Keep small private-helper tests beside module.
- Move broad behavior suites from `app.rs` into
  `native/tests/feature_*.rs` once APIs permit.
- Split `core/src/massive_tests.rs` by service.
- Create shared fixture builders under test-only support modules.
- Avoid one universal app fixture with dozens of implicit defaults.

### Characterization First

Before moving risky behavior, add characterization tests for:

- Overlay close precedence.
- Startup resource restoration.
- Cross-pane navigation.
- PDF generation/pending cleanup.
- Annotation linking and companion notes.
- Search source merging and stale result rejection.
- Save/autosave/index ordering.
- Renderer cursor and selection geometry.

## Error Handling Plan

Introduce layered errors:

```rust
enum VaultError { ... }
enum DatabaseError { ... }
enum PdfError { ... }
enum SearchError { ... }
enum PlatformError { ... }
```

Rules:

- Core errors retain source and operation context.
- Native maps errors to toast/status/log policy.
- User-action failures always surface.
- Cache/background failures may degrade gracefully but remain diagnosable.
- Constructors return `Result`; avoid startup `expect`.
- Thread spawn failure returns service construction error.
- `unwrap`/`expect` remain test-only except documented impossible invariant.

Do not add `anyhow` solely for refactor. Reassess after typed boundaries expose
actual error composition pain.

## Observability And Diagnostics

Extend existing diagnostics surface with:

- Active render/search/highlight generations.
- Pending PDF page/link/text counts.
- Page cache entries and bytes.
- Editor line count and visible line range.
- Layout-cache hit/miss counts.
- Last background error by subsystem.
- Search source durations and result counts.
- Current feature lifecycle states.

Use lightweight counters and timestamps. No logging framework migration required
in same effort.

## Lint And API Hygiene

Gradual policy:

1. Move crate-wide Clippy allowances to smallest module needing them.
2. Remove `dead_code` allowances after feature extraction exposes true usage.
3. Add `#[must_use]` to transition/result values where ignored results are
   dangerous.
4. Limit `pub`; default to private or `pub(crate)`.
5. Document public invariants, especially page/path units and cache lifecycle.
6. Use named input structs when functions exceed seven conceptually distinct
   arguments.
7. Do not chase line-count goals by creating tiny one-function modules.

## Documentation Plan

Maintain:

- `docs/ARCHITECTURE_RULES.md`: enforceable rules only.
- `docs/CODEBASE_RESTRUCTURING_PLAN.md`: migration roadmap.
- `docs/MD_EDITOR_ARCH.md`: resulting editor architecture.
- `docs/PDF_VIEWER_ARCH.md`: resulting PDF architecture.
- `docs/adr/*.md`: decisions and rejected alternatives.

For each completed phase:

- Update path references.
- Mark phase status.
- Record changed invariants.
- Remove obsolete compatibility notes.

## Phased Delivery

### Phase 0: Freeze Baseline

Estimated scope: 2-4 PRs.

- Add characterization tests.
- Capture performance baseline.
- Add ADRs and architecture dependency checks.
- No production moves yet.

Exit gate:

- Critical workflows represented by tests.
- Baseline metrics stored.

### Phase 1: Low-Risk Extractions

Estimated scope: 4-7 PRs.

- Platform desktop integration from `main.rs`.
- Overlay state.
- Tracker state and message reducer.
- Pure renderer geometry.
- Pure PDF link/note helpers grouped under feature.

Exit gate:

- App behavior unchanged.
- First nested feature messages proven.

### Phase 2: Persistence Boundary

Estimated scope: 5-8 PRs.

- Shared database initialization.
- Repositories.
- Remove native SQL.
- Private `AppState` fields.
- Typed core errors.

Exit gate:

- Native no longer depends on `rusqlite`.
- Fresh/upgrade/in-memory migration tests pass.

### Phase 3: Workspace And Search

Estimated scope: 6-10 PRs.

- Workspace state/reducer.
- Vault repository/path resolver.
- Search service/ranking.
- Search state/reducer.
- Parser metadata indexing boundary.

Exit gate:

- Vault/search behavior testable without full UI.
- Reindex remains reliable recovery mechanism.

### Phase 4: Editor Pipeline

Estimated scope: 8-14 PRs.

- Buffer internal split.
- Parser phase split.
- Renderer geometry/measure/draw/hit-test split.
- Editor feature state/reducer.
- Media cache component.

Exit gate:

- Parser and renderer architecture docs match code.
- No performance regression beyond threshold.

### Phase 5: PDF Pipeline

Estimated scope: 8-14 PRs.

- Core PDF domain/infrastructure split.
- Worker scheduling module.
- PDF feature state/reducer/tasks.
- Annotation/search/navigation submodules.
- Cache and generation state machines.

Exit gate:

- `app.rs` contains no PDF implementation details.
- PDF fixture and performance suite passes.

### Phase 6: App Composition Cleanup

Estimated scope: 4-7 PRs.

- Top-level model/update/view/subscription/startup files.
- Final nested messages.
- View models.
- Remove compatibility forwarders.
- Split app tests.

Exit gate:

- `MdEditor` is composition root and cross-feature coordinator.
- Top-level update and view remain readable routing layers.

### Phase 7: Hardening

Estimated scope: 3-6 PRs.

- Remove obsolete lint allowances.
- Tighten visibility.
- Add dependency/file budget CI warnings.
- Refresh all architecture docs.
- Run platform packaging and release signoff.

Exit gate:

- No known boundary violations.
- Release artifacts validated on Linux, Windows, and macOS.

## Pull Request Sizing Rules

Each PR should:

- Move one responsibility or one state cluster.
- Avoid behavior changes unless required by new tests.
- Keep rename/move separate from logic changes where practical.
- Include old-to-new path mapping.
- State invariants preserved.
- Add or relocate tests in same PR.
- Update docs when public architecture changes.

Suggested maximum review size:

- 300-800 changed production lines for logic-bearing PR.
- Larger mechanical moves acceptable when `git diff --color-moved` shows no
  logic change.
- Never combine broad formatting with extraction.

## Validation Gates

Every code-changing PR:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Additional gates by area:

- Editor: source reconstruction, undo/redo, large-doc renderer benchmark.
- PDF: fixture tests, stale generation cleanup, viewport scheduling benchmark.
- Database: fresh schema, migration, in-memory repository tests.
- Vault: temp-vault integration and traversal tests.
- Search: ranking/source/cap/stale-result tests.
- Platform: target compilation or CI matrix.

Release milestone gates:

- `cargo build --release`.
- Linux launch smoke test.
- Windows, Linux, macOS packaging workflows.
- Manual markdown/PDF/split/search/annotation/tracker smoke pass.

## Rollback Strategy

- Preserve compatibility facades for one phase.
- Land state extraction before behavior movement.
- Keep old entry point delegating to new module until tests pass.
- Use feature-level revertable commits.
- Do not change persisted schema and architecture boundary in same PR.
- Database migrations must be forward-only and independently tested.
- If performance regresses, revert latest extraction or retain module boundary
  while restoring prior loop implementation.

## Risk Register

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Giant move creates hidden behavior drift | High | Characterization tests, move-only PRs, compatibility facades |
| Iced lifetime complexity inflates view models | Medium | Start with owned small display values; optimize later |
| Feature reducers duplicate cross-feature state | High | Single source ownership map and coordinator effects |
| Trait-heavy core adds ceremony | Medium | Concrete repositories first; traits only for useful test seams |
| Renderer split regresses hot path | High | Pure extraction order, benchmark each step, borrowed contexts |
| PDF worker move breaks safety | High | Preserve global lock and worker ownership; fixture stress tests |
| Path newtypes cause broad churn | Medium | Introduce at boundaries, migrate inward gradually |
| Database abstraction hides transaction needs | High | Application services own transaction boundaries |
| Long-running branch diverges | High | Continuous small PRs; no mega-branch |
| Docs drift during migration | Medium | Update architecture docs at phase exits |

## Ownership Map

Each subsystem must have one owner:

| Concern | Owner |
| --- | --- |
| Markdown source mutation | `editor/buffer` |
| Markdown grammar and metadata | `editor/parser` |
| Editor visual measurement/draw/hit testing | `editor/renderer` |
| Editor async highlight scheduling | `features/editor` |
| Vault filesystem operations | core filesystem adapter |
| Search ranking and merge | core search application service |
| SQLite schema and queries | core database infrastructure |
| PDFium calls and worker protocol | core PDFium infrastructure |
| PDF UI generations/cache/navigation | `features/pdf` |
| Iced composition and message wiring | `views` and feature presentation |
| Cross-feature actions | top-level app coordinator |
| OS desktop integration | `platform` |

## Completion Metrics

Structural:

- `MdEditor` contains feature states, not 100+ leaf fields.
- Top-level message enum has fewer than 12 feature variants.
- No native production SQL.
- No public core infrastructure mutexes.
- No view receives whole app.
- Parser and renderer remain separate.
- App, renderer, PDF, vault, and state monoliths replaced by cohesive modules.

Quality:

- Required format, Clippy, and test commands stay green.
- Crate-wide lint allowances substantially reduced.
- Critical state transitions have direct tests.
- Error paths surface useful context.
- Architecture docs match source paths and ownership.

Performance:

- Editor draw remains viewport bounded.
- PDF scheduling remains viewport bounded.
- Large-document editing, PDF scrolling, and search stay within agreed baseline
  tolerance.
- Cache limits and invalidation remain explicit and tested.

Delivery:

- Application remains releasable after every phase.
- No migration requires rewriting user vault files.
- SQLite migrations preserve existing user data.

## First Ten Pull Requests

Recommended starting sequence:

1. Add ADR template, dependency rules, and baseline metric document.
2. Add characterization tests for overlay close order and startup restoration.
3. Extract Linux desktop integration from `main.rs`.
4. Extract `OverlayState` with no message changes.
5. Introduce `OverlayMessage`; route through top-level `Message`.
6. Extract `TrackerState`; preserve existing view API.
7. Introduce tracker reducer and move tracker tests.
8. Centralize database schema initialization.
9. Add settings/tracker repositories and make database mutex private for those
   paths.
10. Move native direct SQL behind PDF/search repository APIs and remove native
    `rusqlite`.

These PRs prove migration method before touching editor and PDF hot paths.

## Final Decision Rule

Approve extraction when it improves at least one:

- Ownership clarity.
- Independent testability.
- Dependency direction.
- Invariant enforcement.
- Change isolation.
- Hot-path visibility.

Reject extraction when it only reduces line count while increasing indirection,
generic machinery, allocations, or cross-module hopping.
