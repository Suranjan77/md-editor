# Keyboard Shortcuts

Shortcut behavior follows active context. In split view, click or scroll a pane
before using pane-specific shortcuts.

On macOS, use `Command` where table shows `Ctrl`.

## File And Shell

| Shortcut | Action | Requirement |
| --- | --- | --- |
| `Ctrl+O` | Open vault | None |
| `Ctrl+N` | Create markdown file | Open vault |
| `Ctrl+S` | Save active markdown file | Markdown open |
| `Ctrl+P` | Open command palette | None |
| `Ctrl+B` | Toggle file sidebar | Open vault |
| `Ctrl+T` | Toggle Outline / TOC panel | Markdown or PDF open |
| `Ctrl+Alt+B` | Toggle backlinks | Markdown open |
| `Ctrl+Alt+S` | Toggle study tracker | Open vault |
| `Ctrl+Shift+D` | Toggle diagnostics panel | Open vault |
| `Esc` | Close current modal, palette, search, selection, or panel | Contextual |
| `Tab` / `Shift+Tab` | Move focus forward / backward | Focusable controls |
| `Enter` / `Space` | Activate focused row or control | Focused control |

## Search And Navigation

| Shortcut | Action | Requirement |
| --- | --- | --- |
| `Ctrl+F` | Search active context | Open vault |
| `Ctrl+R` | Open PDF search directly | PDF open |
| `Alt+Left` | Navigate back | Navigation history |
| `Alt+Right` | Navigate forward | Navigation history |
| `Alt+P` | Switch active split pane | Markdown and PDF open |
| `Alt+G` | Follow citation under cursor | Markdown open |
| `Alt+U` | Show usages of active file or reference | Document open |

`Ctrl+F` searches current markdown file when markdown pane is active, current PDF
when PDF pane is active, and vault when no document-local search applies.

## PDF Reading And Annotation

| Shortcut | Action | Requirement |
| --- | --- | --- |
| `Ctrl++` | Zoom in | PDF open |
| `Ctrl+-` | Zoom out | PDF open |
| `Ctrl+0` | Fit PDF | PDF open |
| `Ctrl+G` | Go to page | PDF open |
| `Home` | First PDF page | PDF open |
| `End` | Last PDF page | PDF open |
| `Ctrl+H` | Highlight selected PDF text | PDF selection |
| `Ctrl+Shift+H` | Underline selected PDF text | PDF selection |
| `Ctrl+Alt+H` | Strike out selected PDF text | PDF selection |
| `Alt+N` | Open mapped companion note | PDF with companion note |

PDF page indexes remain internal. UI page labels and generated links use
1-based page numbers.

## Citation Workflow

| Shortcut | Action | Requirement |
| --- | --- | --- |
| `Alt+C` | Open citation palette | Markdown and PDF open |
| `Alt+E` | Toggle excerpt queue mode | Markdown and PDF open |
| `Alt+I` | Insert queued excerpts | Markdown open |

Selection toolbar actions `Quote` and `Cite` are contextual buttons, not
keyboard shortcuts:

- `Quote` inserts selected PDF text plus a `pdf://` source link.
- `Cite` inserts a link to focused PDF annotation.

## Command-Palette-Only Actions

These actions have no default key binding:

- Switch to Dark Theme
- Switch to Light Theme
- Switch to High Contrast Theme
- Toggle Reduced Motion
- Focus Mode
- Split View

Open command palette with `Ctrl+P`, type action name, then press `Enter`.

