# MD Editor V1.0 User Guide

Welcome to the **MD Editor V1.0** — an immersive, robust Markdown environment built to unify beautiful aesthetics with powerful note-taking mechanics. 

## The Digital Vellum Experience
We designed the editor around the "Digital Vellum" theme. By combining soft muted accents, rigorous typographical geometry, and native syntax highlighting, the MD Editor looks less like a terminal and more like an interactive textbook. 
- **Live Preview Widgets**: As you type Markdown (`*italic*`, `**bold**`, or `![image](url)`), the editor seamlessly morphs your raw text directly into rendered, interactive DOM components. 
- **Zero-Shift Interfaces**: Clicking any rendered code snippet smoothly reverts it securely back to raw text for editing without aggressively scrolling or jumping your cursor position.

## Managing Your Vault
The MD Editor is heavily vault-driven. Your "Vault" is simply any standard folder on your local filesystem.
- Click **Open Workspace** (or hit `Ctrl+O`) to select a filesystem directory. 
- Your file list instantly mounts to the left-hand sidebar.
- **Persistent Memory**: The editor will automatically remember your last mounted vault and automatically reopen your last-edited file the next time you launch the application.

## WikiLinks & Cross-Referencing
The application organically maps relationships across your entire vault!
- **Create Links**: Wrap any text in double brackets `[[My Note]]` to create an instantaneous link to `My Note.md`.
- **Navigation**: Click any WikiLink to instantly load that file. 
- **Backlinks Pane**: If enabled `Ctrl+Shift+B`, the Right-Hand sidebar queries the entire filesystem telemetry to instantly show you all *other* documents that link internally back to the document you are currently reading.

## Academic PDF-Style References
Documents often demand structural referencing! Our parser dynamically enumerates blocks natively on the screen:
- **Code**: Automatically prepends sequenced headers (e.g. `Listing 1`, `Listing 2`). 
- **Equations**: Katex structural blocks automatically append right-aligned sequence constants (e.g. `(1)`, `(2)`).
- **Images**: Automatically generates captioned sub-headers (`Figure 1: {alt_text}`).

**Anchor Routing**:
You can link to any of these objects exactly like you would in a PDF! Writing `[See my image](#figure-1)` builds an interactive link that instantly scrolls your viewport down to that exact physical widget when clicked.
