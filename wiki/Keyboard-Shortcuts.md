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

The labels and chords shown in tooltips, menus, the command palette and Settings → Keyboard
all come from one registry, `native/src/shortcuts.rs`; `app/tests.rs`
(`every_registry_chord_triggers_its_shortcut`) checks that each chord there really does what
it says.

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+P` | Command palette |
| `Ctrl+O` | Open a folder as a vault |
| `Ctrl+N` | New note |
| `Ctrl+S` | Save now (the ink page instead while the handwriting canvas is in front) |
| `Ctrl+F` | Find — routed to the active pane, see below |
| `Ctrl+Shift+F` | Search the vault (and the open PDF) |
| `Ctrl+,` | Settings |
| `Ctrl+B` | Toggle the sidebar (bold instead while the editor has focus) |
| `Ctrl+Shift+B` | Toggle backlinks |
| `Ctrl+Shift+O` | Toggle the outline |
| `Ctrl+\` | Split view: note beside the PDF |
| `Ctrl+G` | Knowledge graph |
| `Ctrl+T` | Study tracker |
| `Ctrl+I` | Handwriting canvas (italic instead while the editor has focus) |
| `Ctrl+W` | Close the tab |
| `Ctrl+Shift+T` | Reopen the last closed tab (skips files deleted since) |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Most recently used tab, forward / back |
| `Ctrl+PageDown` / `Ctrl+PageUp` | Next / previous tab in strip order |
| `Ctrl+Shift+PageDown` / `Ctrl+Shift+PageUp` | Move the tab right / left |
| `Ctrl+1` … `Ctrl+8`, `Ctrl+9` | Go to that tab; `Ctrl+9` is always the last |
| `F2` | Rename the selected file |
| `F6` / `Shift+F6` | Move focus between regions: activity rail → sidebar → main → side panel → status bar. Landing on the main region puts the keyboard in the note |
| `F11` | Focus mode (hide the panels) |
| `Ctrl+C` | Copy the PDF text selection, when the PDF pane holds one |
| `Escape` | Close the topmost overlay, in priority order; closing find, the palette or a dialog returns the keyboard to the note |
| `Enter` | Run the palette's highlighted row; open the selected search result; open the node selected in the knowledge graph; otherwise submit the open dialog |
| `↑` / `↓` | In vault search, move the selection; with the sidebar focused, move in the tree (`←`/`→` fold); otherwise scroll the PDF pane or the presentation |
| `Delete` | Remove the lasso selection on the handwriting canvas |
| `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` | Also undo / redo on the handwriting canvas |
| `PageUp` / `PageDown` | Scroll the PDF pane by 520px; in a presentation, previous / next slide |

Right-clicking a file, a tab or the note opens a context menu at the pointer.

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
(or a button, where noted):

| Command | Also a button? |
| :--- | :--- |
| Toggle Backlinks | yes, at the end of the tab strip |
| Study Tracker | yes, on the activity rail |
| Split View | yes, at the end of the tab strip |
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
