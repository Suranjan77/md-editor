# Keyboard Shortcuts

Shortcuts are handled by two layers, and knowing which is which explains the behaviour you
see:

- **Application layer** — `app/subscription.rs`. Sees every key press regardless of focus,
  and returns `None` for anything it does not act on, so ordinary typing does not spawn a
  redundant update cycle.
- **Editor layer** — `editor/renderer/events.rs`. Only fires while the markdown editor widget is
  focused, and handles text editing and formatting.

On macOS, `Cmd` works everywhere `Ctrl` does — both layers test
`modifiers.command() || modifiers.control()`.

---

## Application Layer

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+P` | Open the command palette |
| `Ctrl+O` | Open a folder as a vault |
| `Ctrl+N` | New file |
| `Ctrl+S` | Save the active note immediately, with a confirming toast; saves the ink page instead while the handwriting canvas is in front |
| `Ctrl+F` | Search — routed to the active pane, see below |
| `Ctrl+B` | Toggle the sidebar (bold instead while the editor has focus) |
| `Ctrl+T` | Toggle the table of contents |
| `Ctrl+G` | Open the knowledge graph |
| `Ctrl+I` | Open or close the handwriting canvas (italic instead while the editor has focus) |
| `Ctrl+C` | Copy the PDF text selection, when the PDF pane holds one |
| `Escape` | Close the topmost overlay, in priority order |
| `Enter` | Run the palette's highlighted row; submit global search; open the node selected in the knowledge graph; otherwise submit the open name modal |
| `Delete` | Remove the lasso selection on the handwriting canvas |
| `Ctrl+Z` / `Ctrl+Y` | Also undo / redo on the handwriting canvas (no effect while it is closed) |
| `↑` / `↓` | Scroll the PDF pane or the presentation by 64px |
| `PageUp` / `PageDown` | Scroll the PDF pane by 520px; in a presentation, go to the previous or next slide |

### What `Ctrl+F` does

It follows the active pane rather than opening one fixed panel:

1. **Split view with a file open** — if the PDF pane was the last one you interacted with
   (by scrolling, clicking, or selecting text), it opens PDF search; otherwise it opens
   in-note search.
2. **PDF only** — PDF search.
3. **A note open** — in-note search.
4. **Nothing open** — global vault search.

### What `Escape` closes

In order, closing exactly one thing per press: the lasso selection on the handwriting
canvas → the PDF text selection → the focused annotation → the PDF link preview → the open
modal → the study tracker → in-file search → global search → the command palette → the
knowledge graph → the table of contents.

---

## Editor Layer

Active only while the markdown editor has focus.

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+Z` | Undo one word-sized run |
| `Ctrl+Y` / `Ctrl+Shift+Z` | Redo |
| `Ctrl+A` | Select the whole document |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / cut / paste |
| `Ctrl+B` | Toggle **bold** around the selection |
| `Ctrl+I` | Toggle *italic* |
| `Ctrl+E` | Toggle `inline code` |
| `Ctrl+K` | Insert a link |
| `Tab` | Insert four spaces |
| `Enter` | New line, continuing a list marker where applicable |
| `Backspace` / `Delete` | Delete backward / forward |
| `←` `→` `↑` `↓` | Move the cursor; hold `Shift` to extend the selection |
| `Home` / `End` | Start / end of line; hold `Shift` to extend |
| `Ctrl` + click | Follow a link, including a `pdf://` deep link |

> **The focused editor wins.** Chords the editor acts on (`Ctrl+Z/Y/A/B/I/E/K/C/X/V`) are
> captured by it, so the app-wide meaning of the same chord does not also fire: `Ctrl+B`
> formats bold without toggling the sidebar, `Ctrl+I` formats italic without opening the
> handwriting canvas. Click outside the editor, or use the command palette, for the app
> action. Chords the editor does not bind (`Ctrl+S`, `Ctrl+F`, `Ctrl+P`, …) always reach the
> app (`app/subscription.rs::keyboard_message`).

---

## Command Palette

| Shortcut | Action |
| :--- | :--- |
| `↑` / `↓` | Move the highlighted row |
| `Enter` | Run the highlighted command, or open the highlighted file |
| `Escape` | Close the palette |

Four commands have **no key binding of their own** and are reachable only from the palette
(or the toolbar, where noted):

| Command | Also on the toolbar? |
| :--- | :--- |
| Toggle Backlinks | no |
| Study Tracker | yes |
| Split View | yes |
| Focus Mode | no |

**Focus Mode** is a one-shot action, not a toggle: it hides the sidebar, the backlinks panel,
and the table of contents, and closes the tracker, leaving just the document.

**Split View** requires both a note and a PDF to be open; otherwise it raises a toast
explaining why.

---

## Mouse & Trackpad

| Gesture | Context | Action |
| :--- | :--- | :--- |
| Wheel | Editor / PDF / Presentation | Scroll vertically |
| Wheel over a wide block | Editor | Scroll that table or code block horizontally, independently of the page |
| Click and drag | Editor | Select text |
| Click and drag | PDF | Select PDF text |
| Right-click a reference | PDF | Preview the target equation, figure, table, or section in place |
| `Ctrl` + click a link | Editor | Follow a wikilink, a file link, or a `pdf://` deep link |
| Drag the divider | Split view | Resize the panes |
