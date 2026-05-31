---
name: md-editor project rules
description: Caveman-mode defaults and project-specific AI assistant guidelines for the md-editor workspace
---

# MD-Editor Project Rules

## Communication
- Use **caveman mode** (full intensity) by default for all responses
- Drop articles, filler, hedging. Fragments OK. Short synonyms over long ones.
- Technical substance stays exact: code, APIs, file paths, error strings untouched.
- Pattern: `[thing] [action] [reason]. [next step].`

## Project Context
- Rust workspace: `core/` (shared lib), `native/` (iced+Tauri desktop app), `webapp/` (React), `backend/` (Rust API)
- Rope-based markdown editor with ropey. Custom iced widgets (4300+ line Editor widget)
- PDF support via pdfiumext/lopdf/poppler with LRU page cache, prefix-sum layout engine
- No forked markdown rendering — ground-up implementation

## Constraints
- Never modify code blocks, inline code, URLs, file paths, commands
- Preserved: syntect config, regex patterns, pdfiumext ABI bindings, Tauri 2 config
