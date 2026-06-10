# MD-Editor v3 — Ground-Up Master Plan

**Scale:** 10–20 engineers, 12–18 months.
**Stance:** fresh eyes. This plan designs the product the codebase *wants to be*, not an
incremental cleanup of what exists. The existing repo becomes a quarry: we mine proven
assets (pdfium plumbing, fixtures, tests, design tokens) and leave the architecture behind.
**Relationship to other docs:** supersedes the incremental overhaul plan for any team that
can staff it; the incremental plan remains valid for a 1–2 person effort. ADRs 0001–0004
are re-decided here where evidence demands it.

---

## 1. Why ground-up — the evidence

Three user-visible bugs, all root-caused in this codebase, each pointing at a *structural*
defect no amount of file-splitting fixes:

### Bug A — Ctrl+Z opens "go to page" instead of undoing
`app/subscription.rs` installs a **global** `iced::keyboard::listen()` that maps
Ctrl+Z → `Shortcut::PdfZoomInput` unconditionally, while the editor widget *also* binds
Ctrl+Z → `EditorCommand::Undo` internally (`renderer/widget.rs:2077`). Both fire on every
keystroke; whether you see undo, the zoom input, or both depends on event ordering.
**Structural defect:** two parallel keyboard systems with no focus scoping, no
arbitration, and no single keymap. Every new shortcut is a roll of the dice against every
widget's internal bindings. The command registry exists but is bypassed by both systems.

### Bug B — clicking a line makes it overflow onto the line below
Clicking moves the cursor onto a line → the highlighter reveals concealed syntax markers
(`**`, `#`, `$`) → the line's content gets wider → it wraps to a second visual row → but
the inter-line vertical offsets were computed from the *concealed* layout and are not
reflowed, so the overflow row paints over the next line.
**Structural defect:** layout is a per-line cache keyed by styled-content hash
(`layout_cache.rs`) with no document-level reflow pass when a line's height changes;
conceal/reveal changes layout geometry as a *side effect of styling*. Style and layout are
entangled with no invalidation protocol between them.

### Bug C — a PDF can only be viewed in split view
Opening a PDF link does `split_view_active = true; showing_pdf = true` (`update.rs:110`);
the view tree special-cases "editor, optionally with a second pane". There is no concept
of "a document open in a pane" — there is `workspace.active_path` (markdown) *and*
`pdf.active_path` (PDF) *and* `showing_pdf` *and* `split_view_active` *and* `active_panel`,
five booleans/paths that must be kept mutually consistent by every one of the ~40 call
sites that touch them.
**Structural defect:** no workspace model. Layout is an accident of flags.

These are not three bugs; they are three symptoms of the same disease: **the app has no
kernel** — no input system, no layout protocol, no workspace model. The 2,400-line reducer
and the god-files are downstream of that.

### What is genuinely good (and gets mined, not rewritten)
- pdfium integration incl. thread-safe worker, caching, text extraction (core/infrastructure/pdfium/).
- The rope-based buffer with `EditorCommand` transactional mutation discipline.
- SQLite sidecar + repository pattern; FTS5 search already working.
- 318 green tests, characterization suite, PDF fixture corpus, guardrail scripts, design tokens (all from the 2026-06 stabilization pass).
- The product itself: local-first markdown+PDF+tracker is a real, differentiated niche.

---

## 2. Product definition (the bar)

**One sentence:** a calm, local-first research workspace where Markdown notes, PDFs, and
reading/study workflow are first-class peers — Obsidian-class editing feel, Zotero-class
PDF annotation, zero cloud dependency.

**Pillars (ship gates — a release that violates one slips):**
1. **Typing is sacred.** p95 keypress→frame < 8 ms on a 100k-line document. Undo always
   undoes. No keystroke is ever stolen by an unfocused surface.
2. **Documents are peers.** Any document type opens in any pane, tab, or window. Split is
   a layout choice, never a requirement.
3. **Reading is comfortable.** PDF rendering crisp at any zoom/DPI; annotations survive
   file moves/renames; navigation history works like a browser.
4. **Knowledge compounds.** Links, backlinks, search, and citations are instant and never
   stale; renames repair references.
5. **Local-first, forever-files.** Plain Markdown + standard PDFs + one SQLite sidecar.
   Export everything. No network required, ever.
6. **Calm.** Idle app = zero CPU/redraws. Motion is subtle, fast, and disableable.

**Explicit non-goals (v3):** real-time collaboration, mobile, cloud sync service,
WYSIWYG-without-markdown mode, plugin marketplace (the *API* ships, the marketplace
doesn't).

---

## 3. Architecture from first principles

```
┌────────────────────────────────────────────────────────────────────┐
│  shell (per-OS window mgmt, menus, DnD, file dialogs)              │
├────────────────────────────────────────────────────────────────────┤
│  workspace kernel                                                  │
│   • PaneTree (splits/tabs/windows)  • FocusModel (single owner)    │
│   • InputRouter (one keymap, scope stack, user-remappable)         │
│   • CommandBus (every action is a Command; palette = free)         │
├──────────────┬──────────────────┬──────────────────┬───────────────┤
│ editor engine│  pdf engine      │  graph/search    │  tracker      │
│ (lib crate)  │  (lib crate)     │  (lib crate)     │  (lib crate)  │
├──────────────┴──────────────────┴──────────────────┴───────────────┤
│  vault core: fs watcher · indexer (FTS5) · link graph · annotations│
│  · sessions · config — typed errors, no UI deps                    │
└────────────────────────────────────────────────────────────────────┘
```

### 3.1 Workspace kernel (kills Bug A and Bug C by construction)

- **PaneTree:** a binary split tree of panes; each pane holds a tab strip of *editors*
  (an editor = a view onto a document: MarkdownEditor, PdfViewer, ImageViewer,
  GraphView, …). Document state (buffer, annotations) is owned by a DocumentStore and
  shared by reference — two panes can show the same buffer.
- **FocusModel:** exactly one focused editor at any instant. All input flows through it
  first. Panes render focus visibly.
- **InputRouter:** a single declarative keymap: `(scope, chord) → CommandId`, with scope
  stack `global < workspace < pane < editor-kind < overlay`. Widgets do not bind keys.
  Resolution is innermost-scope-wins; conflicts are *statically detected* at startup and
  in CI (test enumerates the keymap). Ctrl+Z resolves to `editor.undo` in a markdown
  editor scope and `pdf.zoom-input` only in a PDF scope. User remapping is a JSON file
  reusing the same table.
- **CommandBus:** every user action — menu, palette, key, click — dispatches a `CommandId`
  + args. The palette, menus, and docs/SHORTCUTS are *generated* from the registry.
  This is the single most leveraged decision in the plan.

### 3.2 Editor engine (kills Bug B by construction)

- **Text:** keep rope (ropey) + transactional `EditorCommand`s; add persistent undo tree
  (undo history survives restart, sidecar-stored per document hash).
- **Parsing:** incremental block-level parser with explicit block-state entry/exit
  (fences, math, tables), re-parsing forward only to convergence. Evaluate tree-sitter
  (`tree-sitter-markdown`) in a 2-week spike vs evolving the in-house parser; decide by
  ADR-0101 at M1. Selection criteria: incrementality, inline-extension support
  (wikilinks, math, citations), error tolerance.
- **Layout protocol (the Bug-B fix):** three explicit phases with an invalidation
  contract:
  1. *Style* (spans, conceal decisions) — pure, per line, keyed by (text, block state,
     conceal mode).
  2. *Measure* (visual rows, heights) — any height change marks **all subsequent line
     offsets dirty** via a monoid tree (sum-tree of heights → O(log n) offset queries,
     O(log n) invalidation).
  3. *Paint* — viewport-bounded, damage-tracked.
  Conceal is **layout-stable by design**: active-line markers render in muted color
  *within the same measured box* (reserved-width strategy); concealed lines are measured
  in concealed form. The transition is a cross-fade, never a reflow of neighbors
  (golden-test asserted: caret motion produces draw-diffs confined to the two affected
  lines).
- **Editing ergonomics** (from the UX addendum, all command-bus citizens): auto-pairs,
  smart lists/checkboxes with renumbering, table editing, smart paste (URL→link,
  image→vault asset), heading cycling, multi-cursor (the model is `Vec<Selection>` from
  day one), soft-wrap hang-indent.
- **Quality harness:** proptests (apply→undo == identity; cursor in bounds; grapheme
  safety incl. emoji/CJK/CRLF), CommonMark-subset conformance corpus, golden
  draw-command snapshots, p95 latency bench in CI.

### 3.3 PDF engine

- Keep pdfium via the existing core integration (re-affirm ADR-0002).
- **Tile-based renderer:** pages render as zoom-appropriate tiles with an LRU byte-budget
  cache; re-render on zoom-threshold crossings (never upscale > 1.4×); render queue with
  cancellation (offscreen requests dropped).
- **Annotations v2:** keyed by document SHA-256 (survive rename/move); quads + color +
  note + optional link-to-note; numbered SQL migrations; JSON + Markdown-summary
  export/import; optional burn-in export spike.
- **Reading UX:** continuous scroll with virtualization, TOC with current-section
  tracking, browser-grade back/forward, text selection across columns, search overlay,
  page-number pill, "resumed at p. N" restore.

### 3.4 Vault core

- **Watcher-driven truth:** `notify`-based fs watcher (500 ms debounce) keeps tree,
  index, and link graph converged with external edits (test: converge < 2 s).
- **Index:** SQLite FTS5 for markdown + extracted PDF text; incremental by
  mtime+hash diff; cold start with no changes = no reindex.
- **Link graph service:** backlinks, outlinks, broken links, rename-repair as
  transactions.
- **Vault safety:** atomic saves (temp+rename), mtime conflict detection with diff
  prompt, crash journal for unsaved buffers (kill-9 test), `.trash/` soft delete.
- **Typed errors everywhere** (`thiserror`); `Result<_, String>` is banned in v3 crates.

### 3.5 UI toolkit decision (re-open ADR-0001 honestly)

A ground-up rebuild is the one moment a toolkit switch is affordable. Run a **time-boxed
3-week bake-off (M0)** building the same vertical slice (editor pane with conceal +
keymap + a PDF page) in:
1. **iced** (incumbent; team knows it; weakest a11y/text),
2. **gpui** (Zed's toolkit; best-in-class text & GPU perf; youngest ecosystem),
3. **egui** (immediate-mode; fastest iteration; retained-layout friction).
Decision by ADR-0100 with measured latency, LOC, a11y findings. **Default if scores tie:
stay on iced** — porting cost is real and the editor engine is toolkit-agnostic by
construction (draw-command interface), which is the actual insurance policy.

### 3.6 Extension API (seam, not marketplace)

- In-process WASM components (wasmtime) with a capability-scoped API: read/write notes
  via commands, register commands/palette entries, render simple panels (declarative).
- Ship 3 first-party extensions to prove the API: BibTeX citations, tracker analytics
  charts, sample-vault tutorial generator.

---

## 4. Team & workstreams

Six squads; squads own quality gates for their surface. Sizes are steady-state; people
flow toward M-milestone bottlenecks.

| Squad | Size | Owns |
|---|---|---|
| **Kernel** | 3 | PaneTree, FocusModel, InputRouter, CommandBus, session restore, settings |
| **Editor** | 4 | rope/undo, incremental parse, layout protocol, renderer, editing ergonomics, IME |
| **Reading (PDF)** | 3 | pdf engine, tiles, annotations, reading UX |
| **Knowledge** | 3 | vault core, watcher, FTS index, link graph, search UX, citations |
| **Experience** | 2–3 | design system, motion, a11y, i18n scaffolding, onboarding, empty states, UX research |
| **Platform & Quality** | 3 | CI matrix, perf/fuzz infra, packaging (win/mac/linux), crash reporting, release eng |

Cross-cutting roles: 1 architect/tech-lead (owns ADRs, kernel API review), 1 PM/design
lead, QA embedded in Platform & Quality.

---

## 5. Timeline & milestones

18 months, 6 milestones. Every milestone ends with a **usable build** ("eat your own
vault" from M2 onward) and a hard quality gate.

### M0 — Foundations (months 1–2)
- Toolkit bake-off → ADR-0100; parser spike → ADR-0101 (may run into M1).
- Repo: cargo workspace `kernel/ editor/ pdf/ vault/ shell/ xtask/`; CI matrix
  (3 OS), perf bench harness, fuzz targets, golden-test infra from day one.
- Port from v2 quarry: pdfium module, fixtures (+gen script), design tokens, guardrail
  scripts (architecture/budget/unwrap ratchets), contrast tests.
- **Gate:** vertical-slice demo (type with conceal, view a PDF page) at <8 ms p95;
  CI green on 3 OSes.

### M1 — Kernel + editor core (months 3–5)
- PaneTree/tabs/focus; InputRouter with full keymap + conflict CI; CommandBus + palette.
- Editor: rope+undo tree, incremental parse, 3-phase layout with height sum-tree,
  base markdown styling, conceal v1 (layout-stable), selection/multi-cursor model.
- Vault: open/CRUD, watcher, atomic saves.
- **Gate:** Bug-class regression suite green — A: keymap conflict test enumerates all
  scopes×chords; B: golden test "caret enter/leave line ⇒ draw-diff ≤ 2 lines";
  C: PDF opens standalone in a tab (engine may still stub tiles). Dogfood-internal.

### M2 — Reading + knowledge (months 6–8)
- PDF tiles/zoom/TOC/selection/search; annotations v2 with hash keys + migrations.
- FTS5 index incremental + unified search UI; link graph + backlinks + rename repair.
- Session restore; settings UI; theme system on tokens.
- **Gate:** dogfood-default (team's real vaults); index 5k files < 10 s; search < 50 ms;
  annotation survives file rename (test).

### M3 — Editing excellence (months 9–11)
- Ergonomics bundle (pairs/lists/tables/paste/heading-cycling); interactive rendered
  elements (checkboxes, links, images, footnote peek); IME pass; motion system applied
  (caret/scroll/conceal cross-fade, reduce-motion).
- Persistent undo; crash journal; conflict prompts.
- **Gate:** Obsidian-parity checklist (write it M2) ≥ 90%; p95 latency budget holds on
  100k-line doc; external alpha (20 users).

### M4 — Breadth (months 12–14)
- Export: HTML + PDF (print pipeline, ADR); citations (BibTeX) via extension API;
  tracker v2 (charts, CSV) via extension API; annotation export/import; graph view.
- Extension API v1 frozen; 3 first-party extensions shipped on it.
- **Gate:** public beta; crash-free sessions > 99.5%; API semver committed.

### M5 — Polish & v3.0 (months 15–18)
- a11y completeness (keyboard map, contrast CI, screen-reader where toolkit allows —
  honest ADR), i18n string extraction, onboarding/sample vault, empty states.
- Packaging: signed installers + AppImage; update-check (opt-in, no auto-update);
  crash reporting (local-first); docs site; v2→v3 vault migrator (sidecar schema).
- **Gate:** release checklist; perf/memory budgets documented with measured numbers;
  v3.0 tagged.

---

## 6. Quality engineering (always-on, not a phase)

- **Budgets in CI from M0:** keypress p95, draw-diff line count, index times, memory
  ceiling, binary size, dependency count, unwrap=0 (v3 crates), file-size ratchets.
- **Test pyramid:** proptests (buffer, link graph, keymap), conformance corpus
  (markdown), golden draw-command snapshots (renderer), fixture corpus (PDF incl.
  corrupt/CJK/500-page), integration (vault lifecycle, kill-9 recovery), characterization
  ports from v2 where behavior is intentionally preserved.
- **Fuzzing:** parser, PDF open, link parser — nightly.
- **The three bugs become permanent regression tests** (named A/B/C above) before any
  feature work touches their surfaces.

## 7. Risks

| Risk | Mitigation |
|---|---|
| Toolkit bet wrong (gpui churn / iced ceiling) | Editor engine emits toolkit-agnostic draw commands; bake-off is time-boxed with a default |
| Rebuild stalls, v2 users stranded | Milestones are usable builds; v2 maintenance branch gets critical fixes only; vault format is shared so users can switch back |
| Incremental parser underestimated | tree-sitter fallback decided early (ADR-0101); conceal works with either |
| Scope creep via "knowledge graph" ambitions | Non-goals list; PM owns the parity checklist, not feature wishlists |
| 20-person coordination overhead | Kernel API is the only cross-squad contract; ADR discipline; weekly demo cadence |
| pdfium licensing/bundling on signed builds | Resolve in M0 packaging spike, pinned checksums |

## 8. Migration & compatibility

- Vault = plain files: zero migration for notes/PDFs.
- Sidecar: v3 ships a one-shot migrator for annotations/sessions/settings (v2 schema →
  v3, keyed by doc hash); round-trip tested on fixture DBs.
- Keymap: ship a "v2 compatibility" keymap preset.
- The v2 repo stays as `legacy/` reference; nothing imports from it.

## 9. Immediate next actions (week 1)

1. Staff Kernel + Editor seeds (4 people) → M0 bake-off.
2. Freeze v2: critical-fix-only policy (the three bugs above *are* critical — fix A on
   v2 by scoping the global listener with the existing `active_panel`/overlay state;
   fix B by invalidating subsequent line offsets on height change; C needs the v3
   workspace model, document as known limitation).
3. Write the Obsidian/Zotero parity checklists (PM + Experience).
4. Set up the new workspace repo with CI/budgets/fuzz from this plan's M0 list.
