# MD Editor Wiki

Welcome to the official **MD Editor** Wiki. This wiki serves as the definitive reference manual for users, contributors, and systems architects working with the MD Editor project.

MD Editor is a calm, native, local-first desktop workspace for notes, PDFs, images, search, backlinks, and study progress. It is written in Rust and powered by the [Iced](https://github.com/iced-rs/iced) graphical user interface library and Google's [PDFium](https://pdfium.googlesource.com/pdfium/) rendering engine.

---

## System Architecture at a Glance

MD Editor is strictly separated into two crates: a headless engine (`md-editor-core`) and a native graphical desktop interface (`md-editor-native`).

```mermaid
graph TB
    subgraph UI ["md-editor-native (Iced GUI)"]
        App["App State & Update Router<br/>(app.rs, messages.rs)"]
        Canvas["Custom Canvas Markdown Widget<br/>(editor/renderer.rs)"]
        DocBuf["DocBuffer & Undo Runs<br/>(editor/buffer.rs)"]
        Fenwick["HeightTree (Fenwick Tree)<br/>(editor/layout_tree.rs)"]
        Highlighter["Syntax & Typora-style Parser<br/>(editor/highlight.rs)"]
        PDFViewer["Interactive PDF Canvas<br/>(views/interactive_pdf.rs)"]
        Palette["Command Palette & Fuzzy Matcher<br/>(fuzzy.rs, command_palette.rs)"]
        Tokens["Design Tokens & Motion<br/>(theme.rs, motion.rs)"]
    end

    subgraph Core ["md-editor-core (Headless Engine)"]
        AppState["Shared AppState Context<br/>(state.rs)"]
        Vault["Vault Traversal & Atomic Writes<br/>(vault.rs)"]
        Index["Wikilinks & Backlinks Graph<br/>(file_index.rs)"]
        SQLite["SQLite DB (WAL Mode) & Migrations<br/>(state.rs, config.rs)"]
        FTS["FTS5 Full-Text Search Engine"]
        PDFiumWorker["PDFium Worker Thread & Renderer<br/>(pdf.rs)"]
        RefScanner["Heuristic Reference Scanner<br/>(references.rs)"]
        StudyTracker["Study Tracker & Milestones<br/>(tracker.rs)"]
    end

    App -->|Requests / State Sync| AppState
    Canvas -->|Queries Line Offsets| Fenwick
    DocBuf -->|Markdown Text| Highlighter
    Highlighter -->|Styled Spans| Canvas
    PDFViewer -->|Thread Channels| PDFiumWorker
    Vault -->|Atomic Sync| SQLite
    Index -->|Resolves [[links]]| Vault
    AppState --> SQLite
    AppState --> Vault
    AppState --> Index
    AppState --> StudyTracker
```

---

## Wiki Navigation Matrix

| Topic | Description | Link |
| :--- | :--- | :--- |
| **Architecture & Philosophy** | Core tenets: local-first, zero-config portability, atomic durability, non-destructive sidecars | [Architecture & Philosophy](Architecture-and-Philosophy.md) |
| **Repository Structure** | Workspace layout, crate boundaries, modules, and dependency trees | [Repository Structure](Repository-Structure.md) |
| **Core Services** | Database schema, portability paths, vault operations, atomic write protocol, wikilink graph | [Core Services](Core-Services.md) |
| **Native Desktop GUI** | Iced application lifecycle, message loop, state separation, views, and modal overlays | [Native Desktop GUI](Native-Desktop-GUI.md) |
| **Markdown Pipeline** | Rope text buffer, undo/redo runs, auto-pairing, syntax concealing, Fenwick height tree, custom canvas widget | [Markdown Pipeline](Markdown-Pipeline.md) |
| **PDF Engine & Sidecars** | Single-worker PDFium thread, loose search, coordinate mapping, TOC recovery, reference preview, sidecar highlights | [PDF Engine & Sidecars](PDF-Engine-and-Sidecars.md) |
| **Design Tokens & Motion** | 2px spacing grid, strict type scale, single easing curve motion, 0% idle CPU, fuzzy scoring engine | [Design Tokens & Motion](Design-Tokens-Motion-and-Palette.md) |
| **Study Tracker** | Work intervals, pomodoro sessions, reading logs, milestone gates, SQLite domain models | [Study Tracker](Study-Tracker.md) |
| **Data Flows & Invariants** | Keystroke to atomic save, dirty buffer navigation, 5 non-negotiable durability invariants | [Data Flows & Invariants](Data-Flows-and-Durability-Invariants.md) |
| **User Guide & Features** | Complete user manual: vaults, markdown formatting, PDF split-view, search modes, keyboard shortcuts | [User Guide & Features](User-Guide-and-Feature-Manual.md) |
| **Developer Guide & Testing** | Prerequisites, toolchain, PDFium binaries, unit & stress test suites, Linux desktop integration | [Developer Guide & Testing](Developer-Guide-and-Testing.md) |
| **Contributor Guidelines** | Architectural constraints, design token compliance, PR verification checklist | [Contributor Guidelines](Contributor-Guidelines.md) |

---

## Finding What You Need

- **If you are a new user:** Read the [User Guide & Feature Manual](User-Guide-and-Feature-Manual.md) to discover all keyboard shortcuts, split-view reading workflows, search tools, and study tracker features.
- **If you are a developer looking to contribute:** Review the [Contributor Guidelines](Contributor-Guidelines.md), read the [Developer Guide & Testing](Developer-Guide-and-Testing.md), and make sure you understand the [Data Flows & Invariants](Data-Flows-and-Durability-Invariants.md).
- **If you are reviewing the code or systems design:** Explore the deep dives into [Markdown Pipeline](Markdown-Pipeline.md), [PDF Engine & Sidecars](PDF-Engine-and-Sidecars.md), and [Core Services](Core-Services.md).
