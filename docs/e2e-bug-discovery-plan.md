# Bug Discovery Plan — Headless End-to-End Testing

**Goal:** find, reproduce, fix and permanently guard against bugs in the real application,
automatically and without a human at the screen, before and during the
[UI overhaul](ui-overhaul-plan.md).

**Date:** 2026-11-23 · **Status:** Step 0 done except the upstream report (2026-11-24): every
test isolated, quiet-state invariants checked after each step. **Step 1 done:** S1–S17 written
(`e2e/scenarios.rs` for data safety, `e2e/flows.rs` for UI flows; S13 is `#[ignore]`d for
speed and runs before releases). Findings B5–B14 below, all fixed; every hypothesis H1–H10
confirmed-and-fixed or cleared. **Step 2 done:**
`e2e/monkey.rs` — seeded sessions of 17 action kinds (open, type with/without autosave,
caret, undo/redo, save, bold-all, create, rename, delete, external edit, search, palette,
graph, split, Escape) generated from the live state, invariants after every action, delta
shrinking on failure. `cargo test` runs 1×25; a 10×60 exploration (600 actions) found
nothing. Next: Step 4 (goldens) after UI overhaul Phase 0.

---

## 1. Approach

Two pieces of iced's own tooling, used headlessly:

| Piece | What it gives | How it is used here |
| :--- | :--- | :--- |
| **`iced_test` Emulator** | Runs the real `Program` with no window: real `update`, real tasks (vault indexing, PDFium worker, file I/O), real subscriptions (autosave timers, vault watcher), real layout and rendering | `main::program()` is the exact program `main` runs; `e2e::boot()` hands it to the emulator on a throwaway vault |
| **Comet's beacon protocol** (`iced_debug` → `iced_beacon`) | The spans Comet displays: every update with its message, every view, boot | `e2e/telemetry.rs` wraps the program so it emits those spans, and runs the beacon *server* (Comet's half) on a thread to record them. No Comet window, no human |

The Comet GUI itself is not used. If a developer wants it, it can connect to a normal
`debug`-feature build; nothing here depends on it.

### What exists now (`native/src/e2e/`)

- `mod.rs` — `Vault` (temp vault builder with fixtures), `boot()`, and `Session`:
  `settle()` (pump the event loop until quiet), `wait()`, `send(Message)`, `click(text)`,
  `click_at(x, y)`, `type_text()`, `shows(text)`, `screenshot(label)`, `slow_updates()`,
  `finish() -> Probe`.
- `telemetry.rs` — `Traced<P>` program wrapper and the beacon `Recorder`, which appends a
  per-session table (count / p50 / p95 / max per stage, slowest updates) to
  `target/e2e/telemetry.md`.
- `app::Probe` (test-only) — the observable end state: open document, buffer, dirty flag,
  toast, which views are visible, search results, PDF/deck/ink status, vault entries.
- One scenario: `smoke_boots_and_renders_a_note`.
- `core::state::override_data_dir` / `MD_EDITOR_DATA_DIR` — every e2e run uses a private
  settings database.

Run: `cargo test -p md-editor-native e2e -- --nocapture`. Artifacts: `target/e2e/*.png`,
`target/e2e/telemetry.md`.

### Known limits of the harness

| Limit | Consequence | Workaround |
| :--- | :--- | :--- |
| Emulator feeds input events to widgets only, not to subscriptions | Global shortcuts (`Ctrl+F`, `Ctrl+P`…) cannot be pressed as keys | Send `Message::KeyboardShortcut(..)`; key mapping itself is unit-tested in `app/subscription.rs` |
| Emulator state is readable only at the end (`into_state`) | Mid-scenario checks go through the screen (`shows`), the disk, or screenshots | Split long flows into several sessions where a mid-point state matters |
| One emulated window | Opening the ink window switches the emulated window to it | Ink scenarios run the ink window alone; shell assertions come before `InkToggle` |
| Spans cover boot, update and view only | No layout/draw/present timings (those come from the windowed runtime) | Layout cost is covered by the renderer's own `layout_timing` test; add a `Custom` span around `layout_lines` if needed |
| Headless renderer may pick tiny-skia where the desktop uses wgpu | Pixel output can differ slightly from the real window | Screenshots are for review and same-renderer diffs, not cross-renderer comparison |
| `text` widgets need the editor's custom widget to expose text | `shows()` cannot see text inside the Markdown editor widget | Assert editor content through `Probe::buffer` and screenshots |

---

## 2. Findings so far

Found while building the harness. Verified, not hypotheses.

| # | Finding | Severity | Status |
| :--- | :--- | :--- | :--- |
| B1 | **Tests could read the user's real settings.** The settings DB lives beside the executable; for test binaries that is `target/debug/deps/`, and when no DB exists there it is *copied from `~/.local/share/md-editor`*. App tests that construct `MdEditor` could therefore load the user's last vault and history. | High (test isolation, privacy) | **Fixed for all tests (2026-11-24):** `native/src/test_support.rs::isolate_data_dir` runs inside `MdEditor::new_with_startup_file` in test builds (and in `e2e::boot`); core pins a temp dir under `cfg(test)`. Regression test `building_the_app_never_uses_the_real_data_dir`. This became urgent with code-health W10: the default data dir is now `~/.local/share/md-editor`, so unisolated tests briefly created an empty DB there |
| B2 | **`iced_test` 0.14.0: `Emulator::screenshot` takes the UI cache and never returns it**, so any interaction after a screenshot panics (`emulator.rs:290`). | Medium (upstream) | Worked around (own headless renderer); **plan: report upstream with a minimal repro** |
| B3 | A freshly created headless renderer draws no `text`-widget glyphs on its first frame (the editor's custom text draws fine). | Low (upstream/test-only) | Worked around (render two frames); include in the upstream report |
| B5 | **H5 confirmed and fixed (2026-11-24).** `Ctrl+Shift+Z` ran Undo (the editor matched `"z"` without Shift); `Ctrl+B` bolded *and* toggled the sidebar, and `Ctrl+I` italicised *and* opened the handwriting canvas, because the editor never captured the chords it handled. The editor now maps Shift+Z to Redo and captures its chords; `app/subscription.rs::keyboard_message` skips captured Ctrl chords | Medium | Fixed; tests `ctrl_shift_z_redoes_and_ctrl_z_undoes`, `editor_shortcuts_are_captured_…`, `ctrl_shortcuts_a_focused_widget_captured_…` |
| B6 | **H8 confirmed: external edits were lost.** A clean open note did not follow an external edit (the watcher deliberately never reloaded the buffer); a dirty one was overwritten by the next autosave, destroying the external version. | **High** (data loss) | **Fixed 2026-11-24:** `vault::save_file_guarded` keeps the disk version as a conflict copy when it changed since last read/write (`EditorPane::disk_hash`); the watcher reloads a clean open note. Scenarios `s16_*`; core test `guarded_save_keeps_an_external_edit_as_a_conflict_copy`; wiki Invariant 6 |
| B7 | **Renaming a note left bare `[[links]]` to it dangling.** `rename_entry` found backlinks with the index's resolver (basename match) but rewrote them with the plain path resolver (vault-root-relative), so `[[B]]` in a sibling folder never matched `notes/B.md`. Found by S4. | Medium (broken links) | **Fixed 2026-11-24:** `FileIndex::rewrite_links_to` resolves like the index; all rewrites are computed before re-keying. Core test `renaming_a_note_rewrites_bare_links_from_its_sibling_folder` |
| B8 | Rename with unsaved edits in the open note: the rename rewrote links inside it on disk while the buffer held the old text. | Medium | **Fixed 2026-11-24:** rename flushes the open note first and reloads it afterwards |
| B9 | **Plain-text replace-all expanded `$` in the replacement.** With Match Case off, the search was escaped but the replacement went to `regex::replace_all` as a template, so replacing `price` with `$5` deleted the matches (H4, partly). | Medium (silent text loss) | **Fixed 2026-11-24:** `regex::NoExpand`; test `literal_replace_all_inserts_dollar_signs_verbatim` |
| B10 | **A read-only note was silently overwritten.** The atomic write renames a temp file over the target, which needs only a writable folder; permission copying then made the file still *look* read-only. Found by S17. | High (overrides user protection) | **Fixed 2026-11-24:** `write_file` refuses read-only targets; autosave keeps the edits and reports once. Tests `write_file_refuses_a_read_only_note`, `s17_read_only_note` |
| B11 | The tracker accepted `inf` hours (`"inf".parse::<f32>()` is positive) | Low | **Fixed 2026-11-24:** finite check; `s11_tracker_manual_sessions` |
| B12 | **Toasts could vanish almost immediately.** The auto-hide was one `every(3s)` subscription started by the first toast and never restarted, so a toast shown <3 s after another inherited the remainder of its timer. Found as a flaky S6. | Low (missed error messages) | **Fixed 2026-11-24:** the timer is keyed on the toast text (`Subscription::with`) |
| B13 | **Deleted last vault → misleading startup error.** When the remembered vault no longer existed, startup still opened the remembered file, replacing "Not a directory" with "Cannot open X: No vault root set". | Low | **Fixed 2026-11-24:** the last file is restored only once its vault opened; otherwise a toast says the vault was moved or deleted. `s1_remembered_vault_was_deleted` |
| B14 | **PDF page messages had a ~100 ms `Debug`.** `PdfMessage::Rendered` carried a `DynamicImage` whose derived `Debug` prints every pixel; iced formats each message for its debug spans (Comet, e2e telemetry), stalling the update loop per page in debug tooling. | Low (dev tooling) | **Fixed 2026-11-24:** `messages::PageImage` wrapper with a size-only `Debug` |
| P1 | Performance notes (debug build): opening a 20k-line note 228 ms (synchronous full highlight); bold over the whole 20k-line document 190 ms. Keystrokes anywhere in it stay under 100 ms. | Info | Candidates for off-thread highlighting; S13 budgets these at 400 ms |
| H6, H9, H10 | Ink page listed at once after save; per-keystroke update on a 20k-line note within budget; switching away from a PDF mid-render leaves nothing pending | — | **Cleared** by `s12_handwriting_save_and_reopen`, `s13_large_note`, `s8_pdf_open_zoom_and_switch_mid_render` |
| H1 | Tracker `-0.0h` | — | **Cleared:** non-positive hours were already rejected; S11 |
| H2, H3, H7 | Rename of the open dirty note keeps edits and never resurrects the old file; deleting its folder does not recreate it; switching vault flushes first | — | **Cleared** by `s4_rename_open_dirty_note`, `s4_delete_folder_of_open_dirty_note`, `s15_switch_vault_with_dirty_note` |
| B4 | Baseline telemetry (debug build, small vault): boot 158 ms, update ≈ 0.3 ms, view ≈ 0.13 ms. | Info | Recorded in `target/e2e/telemetry.md`; becomes the regression baseline |

---

## 3. Hypotheses to verify first

From the [UI audit](ui-overhaul-plan.md#1-audit--where-the-ui-is-today) and earlier review;
each becomes a scenario that either reproduces it (then fix) or clears it.

| # | Hypothesis | Scenario |
| :--- | :--- | :--- |
| H1 | Tracker shows `-0.0h` for an empty or negative total (`format!("{:.1}h")`) | S11 |
| H2 | Renaming a note that is open leaves the buffer showing the old link text, or autosave writes the stale buffer back over the rewritten file | S4 |
| H3 | Deleting the folder that contains the open note leaves autosave able to recreate it | S4 |
| H4 | Find & replace corrupts `[[wikilinks]]` or code identifiers it shouldn't touch | S6 |
| H5 | `Ctrl+Shift+Z` triggers undo instead of redo; `Ctrl+B` both bolds and toggles the sidebar (`issues.md`) | S2, S14 |
| H6 | Ink page save → page not listed until a re-index (fixed in `cd8fecd`; confirm) | S12 |
| H7 | Switching vault while a note is dirty loses the last keystrokes (flush happens before the root changes — confirm) | S15 |
| H8 | External edit of the open, clean note is not reflected; of a dirty note, silently overwritten | S16 |
| H9 | Large notes: per-keystroke update > 100 ms | S13 |
| H10 | PDF opened then quickly switched away leaves pending pages stuck (render-generation cleanup rule) | S8 |

---

## 4. Plan

### Step 0 — Harden the harness (S)

1. Make every existing app-level test that builds `MdEditor` call the same isolation as
   `e2e::boot` (B1), so no test can touch the user's data.
2. Add per-step **invariant checks** in `Session::settle()` (run after every action, fail
   with the step that broke them):
   - an error toast appears only when the scenario expects one;
   - `Probe::dirty == false` ⇒ disk content equals the buffer for the open note;
   - `Probe::entries` equals what is on disk (supported files, excluded dirs skipped);
   - no PDF page stays in `pending_pages` once settled;
   - no `update` span over the budget (100 ms default, per-scenario override).
3. Report B2/B3 upstream with a minimal counter-app repro.

### Step 1 — Scenario suite (M)

Each scenario: steps → oracles (disk / screen / `Probe` / telemetry) → screenshots.

| ID | Scenario | Hunts |
| :--- | :--- | :--- |
| S1 | Boot on a vault with every file type; boot with a file path; boot with a missing path | Startup, restore, error toasts |
| S2 | Click into the note, type, wait for autosave; undo, redo (`Ctrl+Y` and `Ctrl+Shift+Z`) | Autosave, undo runs, dirty tracking, H5 |
| S3 | Sidebar navigation; wikilink, relative link and heading-anchor navigation; back to a note keeps scroll and undo history | Retained buffers, link resolution |
| S4 | Create / rename / delete file and folder, including the open note and its parent folder | H2, H3, link rewriting, retained-buffer cleanup |
| S5 | Vault search: query, debounce, result click, truncation at 100+ results | Search index, H-truncation |
| S6 | In-note find, regex, case, replace-all, then undo | H4 |
| S7 | Command palette: commands and files, Enter on highlighted row, no-match | Palette routing |
| S8 | PDF: open, render, zoom, fit, page navigation, search, highlight + linked note, switch away mid-render | H10, sidecar writes |
| S9 | Split view: note beside PDF, `Ctrl+F` routing to the last-touched pane | Pane routing |
| S10 | Knowledge graph: open, scope local/global, filters, select, open node, Escape | Graph refresh, navigation |
| S11 | Study tracker: open, start/stop timer, manual session, config edit with invalid JSON | H1, validation |
| S12 | Handwriting: stroke, undo, save (beside note / at root), reopen from sidebar | H6, ink format |
| S13 | Large note (20k lines) and large vault (2k notes): open, type, scroll, search | H9, performance budgets |
| S14 | Formatting commands (bold/italic/code/link/checkbox) on selections | H5, buffer commands |
| S15 | Switch vault with a dirty note; switch to a missing vault | H7 |
| S16 | External changes via the real file watcher: edit, create, delete, rename on disk | H8 |
| S17 | Failure injection: read-only note, non-UTF-8 note, corrupt PDF, unreadable `.pptx`, deleted-while-open | Error paths, toasts, no data loss |

### Step 2 — Randomised sessions (M)

A seeded "monkey" runner: random sequences of 50–200 actions drawn from the scenario
vocabulary (open, type, save, undo, rename, delete, switch, search, toggle views), with the
Step 0 invariants checked after every action. On failure it prints the seed and shrinks the
sequence to a minimal repro (same seed-replay pattern as the renderer's property suites:
`E2E_SEED`, `E2E_CASES`). Default case count stays small for `cargo test`; a larger run
happens before releases.

### Step 3 — Triage and fix loop (ongoing)

For every failure:

1. Reduce to the smallest scenario that reproduces it; commit that as a failing test.
2. Fix at the lowest layer that owns the bug (core before native, state before view).
3. Keep the scenario as a regression test; add a unit test at the fixed layer when
   possible.
4. Record it in a findings table (this document, §2) with severity and commit.

Severity: **High** = data loss, corruption, crash, privacy; **Medium** = broken feature or
wrong result; **Low** = cosmetic or recoverable glitch.

### Step 4 — Visual and performance regression (S, shared with the UI overhaul)

- Screenshots from scenarios become **golden images** once the UI overhaul's Phase 0 lands
  (fonts and tokens first, or every golden would churn). Diffs are reviewed in PRs.
- Telemetry baselines (B4) become budgets: boot, p95 update and p95 view per scenario,
  checked with generous slack in debug builds and tight slack in a `--release` run.

---

## 5. Sequencing with the UI overhaul

| Order | Work | Why here |
| :--- | :--- | :--- |
| 1 | Bug discovery Steps 0–1 (harness hardening, scenario suite) | Fix behavioural bugs on today's UI, before it changes |
| 2 | Bug discovery Step 3 on everything Step 1 finds | Clean baseline |
| 3 | UI overhaul Phase 0 (fonts, tokens, guardrails) | Changes every pixel once |
| 4 | Bug discovery Step 4 (goldens and budgets) | Goldens taken on the new foundations |
| 5 | Bug discovery Step 2 (monkey runner) alongside UI overhaul Phases 1–5 | Each migrated surface is fuzzed as it lands |
| 6 | Every overhaul PR runs the e2e suite; screenshots attached to the PR | Behaviour and visuals guarded together |
