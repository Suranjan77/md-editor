# MD Editor
---
![Home screen](images/home_screen.png)

## Have you used **Tauri**?
If yes, this app is similar. I have a plan to add more by stealing features from **Obsidian**.

---

## How to use it?
- Run `npm install` then `cargo tauri build` from the root folder. You might need to install tauri cli `cargo install tauri-cli`
- Executable file for respective operating system is generated in `src-tauri/target/release/md-editor(.exe)`
- Then figure out.

## Demonstration
![Usage demo](images/demonstration.gif)

---

## Feature List

### Workspace and Files
- Open a vault folder and persist it as the last workspace.
- Browse files and folders in a sidebar tree.
- Create, rename, and delete files/folders from the UI.
- Persist and restore the last opened file between sessions.

### Markdown Editing
- CodeMirror-based markdown editor.
- Save file changes to disk.
- Internal link handling and backlink support.
- Markdown preview-related decorations and rendering support.

### Search and Navigation
- Vault-wide search overlay.
- Backlinks pane for reference discovery.
- Keyboard shortcuts for core actions.

### Split View and Rich Content
- Vertical split view to keep notes and reference content visible together.
- Built-in PDF viewer.
- Image preview for supported image formats.
- Tracker panel for activity/notes workflows.

### Desktop Integration
- Tauri-based desktop shell.
- Native dialogs for folder and file interactions.
- SQLite-backed system configuration storage.
