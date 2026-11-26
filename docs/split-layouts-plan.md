# Split layouts: a two-pane workspace

Status: phases 1–5 done (2026-11-26). Replaces the Note+PDF "split view" with a workspace of up
to two panes, side by side or stacked, with named saved desks on top. Each pane has its own tabs and shows any document:
Note+Note, PDF+PDF or Note+PDF.

## Decisions

- **Two panes at most**, split vertically (side by side) or horizontally (stacked), with a
  draggable divider.
- **Tabs per pane.** Opening a file goes to the focused pane. *Open to the Side* puts it in
  the other pane, creating that pane if needed. A tab can be moved to the other pane, and
  closing a pane moves its tabs into the one that remains, so nothing is lost.
- **One document, one pane.** A file open in one pane is not opened again in the other:
  asking for it focuses the pane that has it. Two live buffers of one note would need
  shared editing state, and keeping each PDF's highlights in one place avoids two copies
  drifting apart.
- **Focus follows interaction.** Clicking, typing or selecting in a pane focuses it.
  Scrolling and resizing do not. The focused pane gets the keyboard shortcuts, the outline,
  backlinks, find and the status bar.

## Architecture: the focused pane's state lives on `MdEditor`

The app's ~650 references to `self.editor`, `self.pdf`, `self.active_path` and friends stay
as they are. They mean "the pane whose state is swapped in". A pane's document state is a
`PaneDocs` bundle:

`tabs`, `active_path`, `editor`, `pdf`, `active_image_path`, `active_image`, `showing_pdf`,
and the image zoom.

`Workspace` holds the other pane's bundle (`other: Option<PaneDocs>`), which pane is
swapped in (`current`), which has focus (`focused`), the axis and the ratio.
`MdEditor::swap_in(id)` exchanges the fields with the bundle. That is a handful of moves,
not copies.

### Messages

- The view builds each pane from its own state (`PaneView`, a set of borrows) and maps the
  pane's messages to `Message::Pane(id, msg)`.
  - Interactions focus the pane: its bundle is swapped in for good, the note it leaves is
    saved, and then `msg` runs as before.
  - Passive messages (scroll positions, measured sizes, caret reports) run with the pane
    swapped in temporarily and leave focus where it is.
- Every task a handler returns is mapped to `Message::ToPane(id, msg)` for the pane it ran
  in. A render, image or crop finishing after focus moved therefore lands in the pane that
  asked for it, again via a temporary swap. If that pane is gone, the message runs in the
  focused pane, because global results such as vault indexing must not be dropped.
  - Pane B's PDF generations start at a large offset, so a result from a closed pane can
    never match the other pane's generation.
- Widget ids (`editor`, the editor and PDF scrollables) are per pane, derived from
  `current`.

### Work that spans both panes

Each of these runs for every pane (`for_each_pane`):

- vault switch and close;
- rename and delete (tabs, open paths, parked buffers);
- watcher reloads of an open note;
- image, crop and sketch refreshes;
- display-scale changes (math, PDF refit);
- quit and session persistence.

Actions that reach across panes look for the pane showing the target:

- linking a PDF highlight to a note;
- Alt+drag crops, which go into the note in the other pane when the PDF is focused;
- `pdf://` links and *Open linked note*, which open in the other pane.

### Saving

Only the focused pane's note can be typed in. Its buffer autosaves as today and is saved
when focus leaves the pane. An edit to the other pane's note (a linked highlight) is saved
in the same step. A dirty buffer is never parked unsaved.

### Session

The tab strip of pane A stays under `tabs`, so old sessions load. Pane B's tabs and the
layout (`axis`, `ratio`, focused pane) are saved beside it.

## Phases

1. **Engine.** `PaneDocs`, `Workspace`, swap and routing, per-pane views and widget ids,
   two tab strips. The old split becomes "Note in A, PDF in B". The shortcut `Ctrl+\`
   splits and unsplits.
2. **Commands and chrome.**
   - Commands: *Split Right*, *Split Down*, *Unsplit*, *Focus Other Pane*, *Move Tab to
     Other Pane*, *Open to the Side* (Ctrl+click in the sidebar, Ctrl+Enter in the
     palette).
   - Chrome: a focus indicator, an empty pane's call to action, and the axis toggle.
3. **Cross-pane features.** Highlight links, crops, `pdf://` links, linked notes, sync
   behaviour, session save and restore.
4. **Tests and docs.** e2e for Note+Note, PDF+PDF, focus, persistence and data safety
   (a dirty note in a pane that loses focus is saved). Wiki: User Guide,
   Native-Desktop-GUI, Keyboard Shortcuts.
5. **Saved desks.** Per-vault named workspace snapshots (`desks.rs`, `app/desks.rs`)
   capturing Pane A/B tabs (with active tab, MRU, and viewer state), split axis, ratio,
   focused pane, and panel visibility under `desks:<vault_root>` in SQLite. Switching desks
   flushes dirty buffers in all panes first (refusing on write failure). Rename/delete
   propagation rewrites or prunes saved desks; externally deleted files are dropped on
   restore (keeping split panes cleanly even if empty). Accessible from the activity rail
   (*Desks*), `Ctrl+Alt+D` (*Desks…*), `Ctrl+Shift+D` (*Save Desk…*), and `Ctrl+P`
   (including `Ctrl+Enter` on a file hit to *Open to the Side*, plus *Split Right*,
   *Split Down*, and *Unsplit*).
