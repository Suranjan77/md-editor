# Core Services (`md-editor-core`)

`md-editor-core` is the foundation of MD Editor: completely headless, it encapsulates
filesystem access, relational persistence, full-text indexing, the wikilink graph, PDFium
rendering, and the study tracker's storage. It compiles and tests without a display server.

---

## 1. Application State & Concurrency (`state.rs`)

```rust
pub struct AppState {
    pub vault_root: Mutex<Option<PathBuf>>,
    pub file_index: Mutex<FileIndex>,
    pub db: Mutex<Connection>,
    pub pdf_renderer: Option<PdfRenderer>,
}
```

- Shared across threads as `Arc<AppState>`.
- Fields use `std::sync::Mutex` with short-lived locks. Slow work — reading files, PDFium
  text extraction — is deliberately performed *before* any lock is taken; see the two-phase
  structure of `set_vault_root` below.
- `AppState::new()` opens the on-disk database; `AppState::new_in_memory()` opens an
  in-memory one with no PDF renderer, which is what the test suite uses.

---

## 2. Relational Database Engine (`state.rs`, `config.rs`)

### Pragmas

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
```

Both are applied best-effort at connection time and fall back silently if unsupported.
WAL keeps writes fast and never blocks a reader behind a writer; `synchronous = NORMAL` is
the recommended pairing with WAL, safe against application crashes.

### Database Schema

```mermaid
erDiagram
    settings {
        TEXT key PK
        TEXT value
    }
    file_search {
        TEXT path "FTS5 virtual table"
        TEXT content
    }
    pdf_documents {
        TEXT document_id PK
        TEXT vault_relative_path
        INTEGER file_size
        INTEGER modified_at
        INTEGER created_at
        INTEGER updated_at
    }
    pdf_annotations {
        TEXT id PK
        TEXT document_id
        INTEGER page_index
        TEXT kind
        TEXT color
        TEXT selected_text
        TEXT ranges_json
        TEXT rects_json
        TEXT note
        TEXT linked_note_path
        TEXT markdown_anchor
        INTEGER created_at
        INTEGER updated_at
    }
    pdf_text_cache {
        TEXT path PK
        INTEGER file_size
        INTEGER modified_at
        TEXT content
    }
    pdf_references {
        TEXT document_id PK
        TEXT links_json
        INTEGER updated_at
    }
    pdf_outline {
        TEXT document_id PK
        TEXT outline_json
        INTEGER updated_at
    }
    tracker_sessions {
        INTEGER id PK
        TEXT date
        REAL hours
        TEXT activity_type
        TEXT phase
        TEXT notes
    }
    tracker_activity {
        INTEGER id PK
        TEXT type
        TEXT text
        TEXT time
    }
    tracker_kv {
        TEXT key PK
        TEXT value
    }

    pdf_documents ||--o{ pdf_annotations : "document_id"
    pdf_documents ||--o| pdf_references : "document_id"
    pdf_documents ||--o| pdf_outline : "document_id"
```

1. **`settings`** — key/value application state. The keys actually written are:

   | Key | Written by | Meaning |
   | :--- | :--- | :--- |
   | `window_size` | `persist_window_size` | `"<width>x<height>"`, re-read before the window exists |
   | `last_vault` | vault open | Absolute path of the vault to restore |
   | `last_file` | file open | Vault-relative path of the document to restore |
   | `scroll:<path>` | `persist_document_position` | Editor scroll offset for that document |
   | `cursor:<path>` | `persist_document_position` | Cursor character offset for that document |
   | `tracker_config` | tracker Config tab | The tracker's JSON curriculum |

2. **`file_search`** — an FTS5 virtual table over `(path, content)`, holding both Markdown
   note text and extracted PDF text, for one vault at a time.
   **`vault_file_meta`** (schema v3) — for each file whose `file_search` row is current:
   `(scope, path, size, modified_ns, links_json)`, the vault scope being the same hash
   `pdf_documents` ids are prefixed with, and `links_json` a note's wikilink targets as
   written. Reopening a vault reads only files whose size or mtime differ (see Vault
   Indexing). Rows and records are cleared together whenever another vault is opened.
3. **`pdf_documents`** — known PDFs by content id, with their vault-relative path, size, and
   timestamps. When a PDF is copied or restored and its id changes, rows recorded for the
   same path *and the same size* are adopted under the new id so annotations are not orphaned.
4. **`pdf_annotations`** — sidecar highlights and notes.
   `ranges_json` holds character-index intervals on the page text layer; `rects_json` holds
   bounding boxes in PDF points; `linked_note_path` points at a Markdown note in the vault.
   Three partial indexes support page lookups and linked-note backlinks.
5. **`pdf_text_cache`** — extracted document text keyed by vault-relative path, invalidated
   by size and mtime, so reopening a vault does not re-run PDFium extraction. Empty results
   are cached too, so scanned PDFs are not retried on every open.
6. **`pdf_references`** — the resolved cross-reference list per document, stored as
   `(RESOLVER_VERSION, links)` JSON so a resolver change transparently invalidates it.
   **`pdf_outline`** (schema v2) — the recovered outline of a bookmark-less PDF, stored as
   `(OUTLINE_VERSION, entries)` the same way; it follows a document-id change like
   annotations and references do.
7. **`tracker_sessions` / `tracker_activity` / `tracker_kv`** — logged study sessions, an
   activity log table, and the tracker's checkbox/status key-values.

### Schema Migrations

Versioning uses SQLite's built-in `PRAGMA user_version`. `init_schema()` runs the full
idempotent DDL (`CREATE TABLE IF NOT EXISTS`) and then `apply_migrations()`, which is where
additive steps belong — each bumping `user_version` — rather than editing the base DDL,
since that only runs for fresh databases.

---

## 3. Data Location (`state.rs`)

```mermaid
flowchart TD
    Start(["settings_db_path"]) --> Override{"MD_EDITOR_DATA_DIR or override_data_dir set?"}
    Override -->|"yes"| Pinned["Use that directory; no migration"]
    Override -->|"no"| Xdg{"XDG_DATA_HOME absolute?"}
    Xdg -->|"yes"| XdgDir["$XDG_DATA_HOME/md-editor"]
    Xdg -->|"no"| Home{"HOME set?"}
    Home -->|"yes"| HomeDir["~/.local/share/md-editor"]
    Home -->|"no"| CurrentDir["Current working directory"]
    XdgDir --> Chosen["Chosen directory"]
    HomeDir --> Chosen
    CurrentDir --> Chosen

    Chosen --> Exists{"Does a database already exist there?"}
    Exists -->|"yes"| OpenDB["Open the connection, apply WAL pragmas, init schema"]
    Exists -->|"no"| Portable{"Database beside the executable (old portable build)?"}
    Portable -->|"yes"| Migrate["Copy the database plus -wal and -shm; leave the original in place"]
    Portable -->|"no"| InitFresh["Create a fresh schema"]
    Migrate --> OpenDB
    InitFresh --> OpenDB
```

- **Data** — `data_dir()`: `$XDG_DATA_HOME/md-editor/`, default `~/.local/share/md-editor/`.
  Relative `XDG_*` values are ignored, per the spec (`xdg_dir`).
- **Log** — `log_dir()`: `$XDG_STATE_HOME/md-editor/`, default `~/.local/state/md-editor/`;
  the override directory when one is set.
- **One-time migration from portable** — `portable_db_path()` (beside the executable) is
  *copied*, not moved, so an interrupted migration cannot lose data. Its `-wal` and `-shm`
  sidecars come along so not-yet-checkpointed writes survive.

---

## 4. Vault Filesystem & Atomic Save Engine (`vault.rs`)

### Traversal Bounds

- **Excluded directories** — `node_modules`, `target`, `build`, `dist`, `__pycache__`,
  `.trash`, plus *every* dotfolder (so `.git`, `.obsidian`, and friends are skipped by the
  `starts_with('.')` check in the walkers).
- **Depth cap** — `MAX_WALK_DEPTH = 32`, which together with a symlink check guards against
  pathological or circular trees without affecting any realistic vault.

### Path Jailing

Every path arriving from a user action or a wikilink passes through
`resolve_vault_path_checked()`, which canonicalizes the candidate and verifies it lies
strictly inside the active `vault_root`. A `../` escape returns an error rather than
touching the file.

### Vault Indexing (`set_vault_root`)

Deliberately two-phase, so the UI thread is never blocked behind disk or PDFium work:

(`vault/indexing.rs`.)

1. **Phase 1 (no locks held)** — one walk (`list_vault_entries`) gives the tree, the notes
   and the PDFs. A note whose size and mtime (ns) match its `vault_file_meta` record is
   not read: its recorded link targets feed `FileIndex::rebuild_from_links`. Changed and
   new notes are read, and their links extracted (`raw_targets`).
2. **Phase 1b (no locks held)** — every PDF's identity is resolved for annotation scoping;
   only new or changed PDFs have their text taken from `pdf_text_cache` or extracted. A
   PDF whose text could not be read gets no record, so it is retried next time (for
   example once PDFium is available).
3. **Phase 2 (short-lived locks)** — publish the vault root; in one transaction delete the
   rows and records of files that are gone and rewrite those of changed files (the
   whole table when this is a different vault, or the first index); swap in the new
   `FileIndex`. `begin_vault_change` keeps the rows when the same vault is reopened and
   clears rows and records together otherwise.

### Atomic Write Protocol

```mermaid
sequenceDiagram
    participant UI as Editor buffer
    participant Vault as vault::write_file
    participant OS as Filesystem and kernel
    participant Disk as Physical storage

    UI->>Vault: save_file(path, content)
    Vault->>OS: Canonicalize the path, resolving symlinks
    Vault->>OS: Read the destination's permissions
    Vault->>OS: Create sibling temp file .name.uuid.tmp
    Vault->>OS: Write the full byte buffer
    Vault->>OS: Apply the destination's permissions
    Vault->>Disk: file.sync_all()
    Disk-->>Vault: Contents are on stable storage
    Vault->>OS: fs::rename(tmp, target) — atomic
    Vault->>Disk: sync_all() on the parent directory, best-effort
    Note over Vault,OS: A reader sees the whole old file or the whole new one
    Vault-->>UI: Ok(())
```

Details worth preserving:

1. **Write through symlinks, not over them.** The path is canonicalized first, so a
   symlinked note updates its target instead of being replaced by a regular file.
2. **Sibling temporary file.** `.{filename}.{uuid}.tmp` lives in the destination's own
   directory, so the rename can never cross a mount point, and the UUID means concurrent
   saves cannot collide.
3. **Permission inheritance.** A note kept at `0600` does not come back world-readable.
   A **read-only** note is refused outright ("… is read-only"): the rename would succeed in
   a writable folder and silently replace a file the user protected. Autosave keeps the
   edits in the buffer and reports the failure once.
4. **Kernel flush before rename.** `sync_all()` on the file, then the rename, then a
   best-effort `sync_all()` on the parent directory so the *directory entry* survives power
   loss too.
5. **Cleanup on failure.** A failed write or rename removes the temporary file and returns
   an error; nothing is left behind and the destination is untouched.

Four unit tests in `vault/tests.rs` cover this directly: content replacement leaving no temp
files, permission preservation, writing through a symlink, and never leaving a truncated
file.

---

## 5. Wikilinks & Backlinks Graph (`file_index.rs`)

### Parsing

```rust
static WIKILINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").unwrap());
```

Compiled once and reused for every file during indexing. It matches both bare
(`[[Note Name]]`) and aliased (`[[Note Name|Display Text]]`) links; only the target is
captured, since the alias affects display, not resolution.

Matches inside code are not links: `file_index/code_spans.rs` finds fenced blocks
(```` ``` ```` / `~~~`, an unclosed fence runs to the end) and inline code spans (a backtick
run closed by an equal run in the same paragraph), and both indexing and rename rewriting
skip any `[[...]]` that starts inside them. So `[[nodiscard]]` in C++ or `[[ -f x ]]` in a
shell snippet never becomes a graph node, and a rename never edits code.

### Resolution Algorithm

`FileIndex::resolve_link(target, file_path)` proceeds in order:

1. **Exact path match** — handles `[[subfolder/Note]]` and root-level `[[Note]]`. A
   path-qualified link (`[[archive/topic]]`, `[[./x]]`) stops here: only its `.markdown`
   twin may stand in, never a same-named note elsewhere (the navigator agrees). A `..` that
   climbs out of the vault resolves to nothing.
2. **Shortest basename match** — for a bare name, look up all known files whose lowercased
   stem matches. `names: HashMap<String, BTreeSet<PathBuf>>` keeps candidates ordered, so
   the shortest path wins deterministically and the link, the click-through, and the
   backlink all agree.
3. **Optimistic forward reference** — when no file with that basename is known yet (a
   dangling link, or a file that has not been indexed), the path-based candidate is kept
   anyway, so creating the note later immediately reveals the incoming backlink.

### Bidirectional Index

```rust
pub struct FileIndex {
    pub outgoing: HashMap<PathBuf, HashSet<PathBuf>>, // file → files it links to
    pub incoming: HashMap<PathBuf, HashSet<PathBuf>>, // file → files that link to it
    vault_root: PathBuf,
    names: HashMap<String, BTreeSet<PathBuf>>,        // lowercased stem → candidate paths
}
```

`incoming` is maintained explicitly rather than derived on demand, so the Backlinks panel is
a map lookup. `rebuild()` runs in two passes — register every filename first, then resolve
links — because a bare `[[Name]]` must be able to reach a file that appears later in the
listing or in another subfolder. `update_file()` does the same incrementally for a single
edit, first removing the file's previous outgoing edges from every target's `incoming` set.
The index keeps each file's link targets as written (`links`), so when a note appears or
disappears, `rebind` re-resolves exactly the links that name it: a dangling `[[Topic]]`
picks up a new `sub/Topic.md`, and links to a deleted note fall back to another note of
that name or dangle again.

### Keeping Links Intact on Rename (`vault/rename.rs`, `vault/relink.rs`)

`rename_entry` plans every link fix *before* moving anything, while links still resolve the
way the user wrote them, then moves the entry and writes the fixed notes. The plan
(`relink::relink_note`, run on each note that is moving or names the entry) covers:

- **Wikilinks** to the renamed file — any type (`.md`, `.pdf`, images, `.ink`) — or to
  anything under a renamed folder, resolved as the index resolves them. A link is rewritten
  to the new vault-relative path (notes without their extension, keeping `#anchor` and
  `|alias`) only when its text would no longer reach the target: a bare `[[note]]` survives a
  folder move, since notes resolve by basename.
- **Markdown links and images** (`[text](path)`, `![alt](path)`, `<...>` and `%20` forms,
  `#`/`?` suffixes kept), resolved relative to the note first (as images load), then to
  the vault.
- **Relative links out of a moved note** (`[[../index]]`, `![](../img/a.png)`), recomputed
  from its new folder.

Links in code, URLs and anchors are left alone. A note that cannot be written (read-only)
keeps its old links and the rename still stands. A folder rename then rebuilds the index and
markdown search rows; a file rename re-indexes just the rewritten notes.

---

## 6. Full-Text Search (`vault/backlinks.rs::search_vault`)

Each whitespace-separated term is quoted (embedded quotes doubled) so it is a literal FTS5
string, not query syntax; every term must match, and the **last one matches as a prefix**
(`"trans"*`), so results arrive while a word is still being typed:

```sql
SELECT path, snippet(file_search, 1, '<b>', '</b>', '...', 15), content
FROM file_search
WHERE file_search MATCH ?1               -- path and content
ORDER BY bm25(file_search, 10.0, 1.0)    -- a hit in the path outranks one in the body
LIMIT 101;
```

Both Markdown notes and extracted PDF text are indexed into the same table. Each result
carries its path, a highlighted snippet and a `line`: the 1-based line of the first match in
a note (so opening the hit can select it), 0 for PDF text. **PDF quick notes**
(`pdf_annotations.note`, this vault's documents only) are matched too — every term, case
insensitively — and reported with their 1-based page and the note as context. Results are
capped at 100, with `truncated` set when there were more.

---

## 7. Backlinks Beyond Markdown (`vault.rs::get_mixed_backlinks`)

Backlinks are not limited to note-to-note wikilinks. `get_mixed_backlinks(state, path)`
branches on the active document's type and returns `BacklinkItem`s whose `source` is a
`BacklinkTarget`:

**When the active document is a Markdown note:**

- `MarkdownFile { path }` — every note whose `[[wikilink]]` resolves to this one, taken
  from `FileIndex::get_backlinks`;
- `PdfAnnotation { document_path, annotation_id, page }` — every sidecar highlight whose
  `linked_note_path` equals this note, joined from `pdf_annotations` to `pdf_documents`.
  Activating one opens that PDF at the exact page and focuses the highlight.

**When the active document is a PDF:**

- `MarkdownFile { path }` — notes that wikilink to the PDF itself;
- `MarkdownFile { path }` with the highlight's `selected_text` as context — notes reached
  through this PDF's own annotations, so the panel shows which passage produced the link.

The `BacklinkTarget::PdfDocument { path }` variant is defined in `core/src/types.rs` and
rendered by `views/backlinks.rs`, but no query currently emits it.
