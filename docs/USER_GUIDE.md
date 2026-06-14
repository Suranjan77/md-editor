# User Guide

MD Editor is a local-first workspace for Markdown notes and PDFs. You bring a
folder; the app gives you an editor, file tree, PDF reader, search, backlinks,
and a study tracker around it. Nothing leaves your machine, and everything stays
as plain files you can use elsewhere.

For the full key map, press `Ctrl+/` in the app or see
[SHORTCUTS.md](SHORTCUTS.md). A few shortcuts below differ from what you might
expect — the command palette is `Ctrl+Shift+P`, and `Ctrl+P` is quick-open.

## Opening a vault

A *vault* is any folder on disk. On first launch you choose one; the file panel
then indexes every supported file beneath it:

| Type | Extensions |
|---|---|
| Markdown | `.md`, `.markdown` |
| PDF | `.pdf` |
| Images | `.png`, `.jpg`, `.jpeg`, `.gif`, `.bmp`, `.webp` |

Use **Open Vault Folder** (command palette) to switch vaults. Recently opened
vaults are remembered.

## The file panel

Toggle it with `Ctrl+B`. From the panel (or the command palette) you can create
a **New Note** or **New Folder**, **Rename**, and **Delete**. Click a file to
open it in the focused pane; folders expand and collapse in place.

## Writing Markdown

The editor renders Markdown live while you type — headings grow, emphasis
styles, and markers stay hidden until your caret enters the text that owns them.

Supported: headings, bold/italic/inline code, links and wikilinks, blockquotes,
bullet and task lists, tables, fenced code with syntax highlighting, images, and
math.

Common actions:

| Action | Shortcut |
|---|---|
| Save | `Ctrl+S` |
| Undo / Redo | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Find in note | `Ctrl+F` |
| Select all / Copy / Cut | `Ctrl+A` / `Ctrl+C` / `Ctrl+X` |
| Heading level 1–6 | `Ctrl+1` … `Ctrl+6` |
| Toggle checkbox | `Ctrl+Enter` |
| Outline / Backlinks | palette / `Ctrl+Shift+B` |

Formatting commands without a default key (Bold, Italic, Inline Code, Bullet
List, Wikilink, Heading Cycle) are available from the command palette
(`Ctrl+Shift+P`) and can be bound to keys — see [Custom keymaps](#custom-keymaps).

Undo is a **tree**: editing after an undo branches history rather than throwing
away your redo path, so you never lose work by typing in the wrong place.

## Reading PDFs

Open a PDF like any other file; it renders inside the app with continuous pages
and fit-to-width viewing.

| Action | Shortcut |
|---|---|
| Zoom in / out | `Ctrl+=` / `Ctrl+-` |
| Set exact zoom | `Ctrl+Z` (in a PDF pane) |
| Go to page | `Ctrl+G` |
| Find in PDF | `Ctrl+F` |
| Table of contents | `Ctrl+T` |
| Highlight selection | `Ctrl+H` |
| Edit annotation note | `Ctrl+N` |
| Copy selection | `Ctrl+C` |
| Back / Forward (jump history) | `Alt+Left` / `Alt+Right` |

Highlights and notes are stored in a **sidecar** database, not written into the
PDF, so the original file is never modified. Annotations are keyed by content
hash, so they survive renaming or moving the PDF. You can cycle a highlight's
color, open a linked Markdown note for a passage, export all annotations to
Markdown, and run an **Orphaned Annotations Report** if a PDF's contents change.

## Working side by side

Split the workspace with `Ctrl+\` (split right) to read a PDF beside a note, or
keep several documents as tabs in a pane. Any document type can open in any
pane — a PDF does not force split view. Each pane keeps its own search context,
so `Ctrl+F` always searches the surface you're focused on.

| Action | Shortcut |
|---|---|
| Split right | `Ctrl+\` |
| Close tab / pane | `Ctrl+W` / palette |
| Next tab | `Ctrl+Tab` |

## Searching

- `Ctrl+F` — find within the focused note or PDF.
- `Ctrl+Shift+F` — search the whole vault, including text extracted from PDFs.
- `Ctrl+P` — quick-open a file by name.
- `Ctrl+Shift+P` — command palette (every command, filterable, with its shortcut).

## Study tracker

Toggle the tracker panel with `Ctrl+Shift+T`. Record study sessions, log
reading, set project stages and gates, and edit its configuration — all stored
in your vault alongside your notes, useful when your notes are part of an ongoing
study or research routine.

## Settings and session

- `Ctrl+,` opens Settings (including a *Reduced motion* option).
- Your layout, open tabs, focus, and each PDF's page and zoom are saved on exit
  and restored on next launch.

## Custom keymaps

Key bindings can be overridden per vault by creating
`<vault>/.md-editor/keymap.json`. Invalid entries are reported as warnings at
startup and ignored; they never block launch. The authoritative list of command
IDs and default bindings is [SHORTCUTS.md](SHORTCUTS.md), generated from the
command registry.

## Where your data lives

Your notes, PDFs, and images stay exactly where you put them. App-managed state
for a vault lives under `<vault>/.md-editor/` (search index, annotations,
session, keymap overrides) and can be safely deleted — it will be rebuilt. See
the [README](../README.md#a-local-first-promise) for portable vs. installed
configuration locations.
