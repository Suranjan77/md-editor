# Data Flows & Durability Invariants

MD Editor is built around reliability guarantees that are checked, not assumed. This page
traces the end-to-end flows and states the five invariants every build must satisfy.

---

## 1. End-to-End Data Flows

### Flow 1: Keystroke to Atomic Save

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant App as Iced event loop — app.rs
    participant Buf as DocBuffer — buffer.rs
    participant Tree as HeightTree — layout_tree.rs
    participant Sub as Autosave subscription
    participant Vault as vault::save_file
    participant SQLite as SQLite connection
    participant Disk as Filesystem and storage

    User->>App: Types a character
    App->>Buf: execute(TypePaired or InsertText)
    Buf->>Buf: Insert into the ropey::Rope
    Buf->>Buf: Coalesce into the current undo run if it continues one
    App->>App: Re-highlight, synchronously or debounced by document size
    App->>Tree: Invalidate the edited line's cached height
    App->>App: Set autosave_pending_since = Instant::now

    loop every AUTOSAVE_POLL — 100ms
        Sub->>App: AutosaveElapsed
        Note over App,Sub: Write only once 400ms have passed since the last keystroke
    end

    App->>Vault: save_file(vault_root, rel_path, content)
    Vault->>Disk: Write to the sibling temp file
    Vault->>Disk: Copy the destination's permissions
    Vault->>Disk: file.sync_all
    Disk-->>Vault: Contents are on stable storage
    Vault->>Disk: Atomic rename over the destination
    Vault->>Disk: sync_all on the parent directory, best-effort
    Vault-->>App: Ok
    App->>SQLite: Persist scroll and cursor offsets for this path
    App->>App: Clear the pending flag. Ctrl+S also raises a confirming toast
```

An explicit `Ctrl+S` takes the same path but always answers, even when autosave had already
committed. A **failed** autosave raises a toast the first time it happens in a run of
failures — a failing write is exactly when the user needs to know — and the title keeps its
unsaved marker while the debounce restarts.

### Flow 2: Note Navigation & Undo Retention

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant UI as Sidebar
    participant App as app.rs
    participant Retain as Retained buffers — 32, LRU
    participant Vault as vault.rs

    User->>UI: Clicks Lecture2.md
    UI->>App: SidebarFileClicked

    alt The active buffer has uncommitted edits
        App->>Vault: Flush the active note to disk now
        Note over App,Vault: If the write fails, navigation is refused and reported
    end

    App->>Retain: Park the outgoing DocBuffer with its undo stack and scroll offset
    App->>App: Persist scroll and cursor offsets for the outgoing path

    alt Lecture2.md is retained and its disk content still matches
        Retain-->>App: Restore the buffer with its full undo history
    else Not retained, or the file changed on disk
        App->>Vault: Read the file from disk
        Vault-->>App: Text content
        App->>App: Build a fresh DocBuffer
    end

    App->>App: Restore the stored scroll and cursor offsets
    App->>App: Refresh backlinks and the table of contents
```

The retained registry holds `MAX_RETAINED_BUFFERS = 32` documents, evicting the least
recently used. `take_retained_matching(path, disk_text)` only returns a parked buffer when
the file on disk still matches what the buffer was based on — otherwise the stale buffer is
dropped, so an external edit is never silently overwritten by a resurrected undo stack.

### Flow 3: PDF Selection to Sidecar Highlight & Linked Note

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant Canvas as Interactive PDF widget
    participant App as app.rs
    participant Core as AppState
    participant DB as SQLite — pdf_annotations
    participant Modal as Link note picker

    User->>Canvas: Drags across text on page 7
    Canvas->>Canvas: Hit-test the text layer and compute the text-index range
    Canvas->>App: PdfSelectionChanged, then PdfSelectionFinished
    User->>App: Chooses Highlight
    App->>App: Merge character rects into line rectangles in PDF points
    App->>App: Take next_highlight_color, then advance the palette
    App->>Core: save_pdf_annotation(annotation)
    Core->>DB: INSERT INTO pdf_annotations
    App->>Canvas: Redraw the annotation overlay layer

    opt The user chooses Link to Note
        App->>Modal: Open the searchable vault picker
        User->>Modal: Picks an existing note, or a folder plus a new name
        Modal->>App: NameModalSubmit
        App->>App: Append a Page N section, or create the note with frontmatter
        App->>Core: Update the annotation's linked_note_path
        Core->>DB: UPDATE pdf_annotations
    end
```

---

## 2. The Five Non-Negotiable Durability Invariants

These are pass/fail engineering requirements, not judgement calls. **If any one fails, the
build does not ship, regardless of what else works.**

### Invariant 1: Kill it mid-edit

- **Scenario** — type paragraphs into a note, then `kill -9` the process without pressing
  `Ctrl+S`.
- **Guarantee** — reopening the vault shows no corrupted characters and no truncated or
  empty file. The sibling-temp-file protocol means the destination holds either the complete
  previous version or the complete new one, never a mix.
- **Mechanism** — `vault::write_file`, plus a unit test
  (`write_file_never_leaves_a_truncated_file`) that asserts it directly.

### Invariant 2: Switch with unsaved edits

- **Scenario** — edit a note and immediately click another file, an image, or close the
  window, before the 400ms debounce fires.
- **Guarantee** — the buffer is flushed **before** the view changes. If the write fails —
  disk full, permission revoked — navigation is refused and the error is surfaced, so
  unsaved edits are never abandoned. Window close is the one deliberate exception: it flushes
  best-effort and does not block, because an app that refuses to quit is a worse outcome.

### Invariant 3: Undo is word-sized

- **Scenario** — type a multi-word sentence, press `Ctrl+Z` once.
- **Guarantee** — a run of typing disappears, not a single character.
- **Mechanism** — `UNDO_COALESCE_WINDOW = 300ms` plus `can_coalesce`, which breaks a run at a
  pause, a newline, a cursor jump, a direction change, or a selection edit.

### Invariant 4: Navigation preserves undo

- **Scenario** — edit `FileA.md`, go to `FileB.md`, edit it, come back to `FileA.md`, press
  `Ctrl+Z`.
- **Guarantee** — undo walks back through the edits made before navigating away, and the
  cursor and scroll position are where you left them.
- **Mechanism** — the 32-document retained registry, invalidated when the file changed on
  disk in the meantime.

### Invariant 5: Zero idle CPU

- **Scenario** — leave the app open with no typing, no mouse movement, and no animation in
  flight.
- **Guarantee** — the process consumes **0.0% CPU**.
- **Mechanism** — the per-frame subscription is armed only while `motion.is_animating()`
  holds; every other subscription (toast, autosave, highlight, search) is likewise armed only
  when it has pending work. A settled window has no live timers at all.

Each invariant has a manual verification step in the
[Release Checklist](Release-Checklist.md).
