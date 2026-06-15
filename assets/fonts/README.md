# Vendored fonts

The Quiet Vault design system mandates two typefaces (see `docs/DESIGN-SYSTEM.md §2`).
Both are vendored here so the app embeds them directly instead of relying on system fonts.

| File | Family | Role | Axis |
|---|---|---|---|
| `HankenGrotesk-VariableFont_wght.ttf` | Hanken Grotesk | UI + editor body | `wght` 100–900 (uses 400/500/600/700) |
| `GeistMono-VariableFont_wght.ttf` | Geist Mono | code, numerals, keycaps, line/col | `wght` 100–900 (uses 400/500) |

These are **variable fonts** — a single file covers every weight the design calls for, so
the weight is selected at render time (no separate static files needed).

## Source & license

Both files are downloaded from the **`google/fonts`** repository and licensed under the
**SIL Open Font License 1.1** (OFL). The full license text for each family is kept alongside
the font:

- `HankenGrotesk-OFL.txt` — from `google/fonts/ofl/hankengrotesk/OFL.txt`
- `GeistMono-OFL.txt` — from `google/fonts/ofl/geistmono/OFL.txt`

Upstream:
- Hanken Grotesk — https://github.com/google/fonts/tree/main/ofl/hankengrotesk
- Geist Mono — https://github.com/google/fonts/tree/main/ofl/geistmono (Geist by Vercel)

## Wiring (planned — see `docs/QUIET-VAULT-MIGRATION.md` Phase 2)

Embed via Iced's application builder, e.g.:

```rust
iced::application(/* … */)
    .font(include_bytes!("../../../assets/fonts/HankenGrotesk-VariableFont_wght.ttf").as_slice())
    .font(include_bytes!("../../../assets/fonts/GeistMono-VariableFont_wght.ttf").as_slice())
    .default_font(iced::Font::with_name("Hanken Grotesk"))
```

Then map `FontRole::Mono` → `Geist Mono` and the sans/bold/italic roles → `Hanken Grotesk`
in `shell/src/gui/editor_canvas.rs`.
