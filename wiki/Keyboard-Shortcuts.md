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
| `Ctrl+D` | Open or synthesize today's Daily Desk (`daily/YYYY-MM-DD.md`) |
| `Ctrl+=` | Evaluate selected math expression or enclosing `$...$` / `$$...$$` inline (`= result`) |
| `Ctrl+Alt+R` | Run the enclosing `sh` / `bash` / `python` fenced code block (opt-in, 5s timeout) and insert/update `> [!output]` |
| `Ctrl+S` | Save now (the ink page instead while the handwriting canvas is in front) |
| `Ctrl+F` | Find — routed to the active pane, see below |
| `Ctrl+Shift+F` | Search the vault (and the open PDF) |
| `Ctrl+,` | Settings |
| `Ctrl+B` | Toggle the sidebar (bold instead while the editor has focus) |
| `Ctrl+Shift+B` | Toggle backlinks |
| `Ctrl+Shift+O` | Toggle the outline |
| `Ctrl+\` | Split into two panes / close the second pane |
| `Ctrl+J` | Focus the other pane (splitting if there is one pane) |
| `Ctrl+Shift+M` | Move the tab to the other pane |
| `Ctrl+Shift+L` | Panes side by side ⇄ stacked |
| `Ctrl+Shift+D` | Save current workspace as a desk |
| `Ctrl+Alt+D` | Open Saved Desks dialog |
| `Ctrl+G` | Knowledge graph |
| `Ctrl+T` | Study tracker |
| `Ctrl+I` | Handwriting canvas (italic instead while the editor has focus) |
| `Ctrl+Shift+I` | Insert a sketch into the open note and draw it in the ink window |
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
| `Tab` / `Shift+Tab`, `←` / `→`, `P` | When the knowledge graph is open (and no search field/modal holds focus): step through visible nodes (`Tab` / `Shift+Tab` / `←` / `→`) and pin or unpin the selected node (`P`) |
| `↑` / `↓` | In vault search, move the selection; with the sidebar focused, move in the tree (`←`/`→` fold); otherwise scroll the PDF pane |
| `Delete` | Remove the lasso selection on the handwriting canvas |
| `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` | Also undo / redo on the handwriting canvas |
| `PageUp` / `PageDown` | Scroll the PDF pane by 520px (in a note, the editor moves the caret instead) |

Right-clicking a file, a tab or the note opens a context menu at the pointer.

### What `Ctrl+F` does

It follows the active pane rather than opening one fixed panel:

1. **Two panes** — it searches the focused pane: the pane last clicked, typed or selected
   in. Scrolling a pane does not focus it.
2. **A PDF in front** — PDF search.
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
| `Tab` | Insert four spaces; with a selection across lines, indent each non-blank line |
| `Shift+Tab` | Outdent the selected lines (or the caret's line) by four spaces or a tab |
| `Enter` | New line, continuing a list marker where applicable |
| `Backspace` / `Delete` | Delete backward / forward |
| `Ctrl+Backspace` / `Ctrl+Delete` | Delete to the start / end of the word (one undo step) |
| `←` `→` `↑` `↓` | Move the cursor; hold `Shift` to extend the selection |
| `Ctrl+←` / `Ctrl+→` | Previous / next word; hold `Shift` to extend |
| `Home` / `End` | Start / end of line; hold `Shift` to extend |
| `Ctrl+Home` / `Ctrl+End` | Start / end of the note; hold `Shift` to extend |
| `PageUp` / `PageDown` | Move the caret one screen up / down, keeping its column; hold `Shift` to extend |
| `Ctrl` + click | Follow a link, including a `pdf://` deep link |
| `[[` … / `@` … / `/` at a line start | Open completion: vault files and `#heading` anchors / `.bib` and PDF citations / blocks to insert |
| `↑` / `↓`, `Enter` or `Tab`, `Escape` | With completion open: choose, insert, close |

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
| `Enter` | Run the highlighted command, open the highlighted file, or switch to the highlighted desk |
| `Ctrl+Enter` | Open the highlighted file to the side in the other pane |
| `Escape` | Close the palette |

Commands with **no key binding of their own** are reachable from the palette (or a button,
where noted):

| Command | Also a button? |
| :--- | :--- |
| Insert Image… | yes, in the editor right-click menu |
| Split Right, Split Down, Unsplit | yes, the split / unsplit button on the tab strip |

**Focus Mode** is a one-shot action, not a toggle: it hides the sidebar, the backlinks panel,
and the table of contents, and closes the tracker, leaving just the document.

**Split / Unsplit** works with anything open. A new pane takes the companion document (a
PDF beside a note, a note beside a PDF) when there is one, and otherwise starts empty.

---

## Mouse & Trackpad

| Gesture | Context | Action |
| :--- | :--- | :--- |
| Wheel | Editor / PDF | Scroll vertically |
| Wheel over a wide block | Editor | Scroll that table or code block horizontally, independently of the page |
| Click and drag | Editor | Select text |
| Double-click / triple-click | Editor | Select the word / the whole line |
| Click and drag | PDF | Select PDF text |
| Right-click a reference | PDF | Preview the target equation, figure, table, or section in place |
| `Ctrl` + click a link | Editor | Follow a wikilink, a file link, or a `pdf://` deep link |
| Drag the divider | Two panes | Resize the panes (double-click: back to half and half) |
| Click / type / select in a pane | Two panes | Focus that pane |
