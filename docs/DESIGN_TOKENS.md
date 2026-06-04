# UI Design Tokens and System Specifications

This document defines the core styling variables, themes, component state matrices, and visual standards for the Markdown Editor. Every view, component, and style helper must adhere to these tokens.

---

## 1. Color System & Themes

We support three color themes, each designed to meet WCAG contrast guidelines and align with the application's dense, research-focused workflow.

### A. MD Editor Premium Dark (Default)
A low-fatigue, premium dark interface leveraging deep slate and teal-mint accents.

| Token | CSS/HEX Equivalent | Role |
| --- | --- | --- |
| `BG_PRIMARY` | `#0d0e10` | Main application background (sidebar, editor workspace) |
| `BG_SECONDARY` | `#181a1d` | Secondary background (toolbars, active tool windows, lists) |
| `BG_TERTIARY` | `#23262b` | Tertiary background (nested lists, code blocks, cards) |
| `BG_SURFACE` | `#334b47` | Surface highlighting / active block highlights |
| `BORDER` | `#45484e` | Standard borders and dividers |
| `BORDER_SUBTLE` | `#1d2024` | Faint container splits |
| `TEXT_PRIMARY` | `#e3e5ed` | High-contrast text / body reading text |
| `TEXT_SECONDARY`| `#a9abb2` | Secondary labels, timestamps, metadata |
| `TEXT_MUTED` | `#9d9ea3` | Placeholders, hotkey hints, disabled labels |
| `ACCENT` | `#b1ccc6` | Primary teal-mint accent (active states, focus, cursors) |
| `ACCENT_SECONDARY`| `#cde8e2` | Hover accent, primary highlight |
| `ACCENT_GLOW` | `#b1ccc6` (50% opacity) | Pulse indicator, selection highlight glow |
| `ACCENT_DIM` | `#b1ccc6` (20% opacity) | Selection range backgrounds, inline link markers |
| `DANGER` | `#ee7d77` | Strikeouts, errors, destructive alerts |
| `SUCCESS` | `#d9f2d2` | Saved state, positive indicators, completions |
| `WARNING` | `#bfdad4` | Warnings, intermediate indicators |

### B. MD Editor Premium Light
A clean, soft-contrast light interface using cool grays and forest-teal accents.

| Token | CSS/HEX Equivalent | Role |
| --- | --- | --- |
| `BG_PRIMARY` | `#f7f8fa` | Main background |
| `BG_SECONDARY` | `#edf0f2` | Secondary panel backgrounds |
| `BG_TERTIARY` | `#e1e4e6` | Embedded code blocks, inner panels |
| `BG_SURFACE` | `#d0e1db` | Surface highlights |
| `BORDER` | `#b8c0c5` | Main borders |
| `BORDER_SUBTLE` | `#d8dee1` | Secondary borders |
| `TEXT_PRIMARY` | `#1b1e22` | Body text |
| `TEXT_SECONDARY`| `#484f56` | Secondary labels |
| `TEXT_MUTED` | `#768087` | Muted hints / placeholder |
| `ACCENT` | `#2e5c54` | Rich forest-teal accent |
| `ACCENT_SECONDARY`| `#417d72` | Hover accent |
| `ACCENT_GLOW` | `#2e5c54` (50% opacity) | Focus ring glow |
| `ACCENT_DIM` | `#2e5c54` (20% opacity) | Link accent / selection highlights |
| `DANGER` | `#c0392b` | Error / delete indicators |
| `SUCCESS` | `#27ae60` | Success indicators |
| `WARNING` | `#e67e22` | Warning highlights |

### C. MD Editor High Contrast
A stark, high-contrast theme optimized for accessibility and low-vision users.

| Token | CSS/HEX Equivalent | Role |
| --- | --- | --- |
| `BG_PRIMARY` | `#000000` | Pure black background |
| `BG_SECONDARY` | `#121212` | Dark gray panel backgrounds |
| `BG_TERTIARY` | `#242424` | Selection targets |
| `BG_SURFACE` | `#003300` | Heavy highlight background |
| `BORDER` | `#ffffff` | Stark white borders |
| `BORDER_SUBTLE` | `#cccccc` | Stark off-white borders |
| `TEXT_PRIMARY` | `#ffffff` | Pure white text |
| `TEXT_SECONDARY`| `#e0e0e0` | High-contrast secondary text |
| `TEXT_MUTED` | `#aaaaaa` | Muted text |
| `ACCENT` | `#00ffff` | Bright cyan accent |
| `ACCENT_SECONDARY`| `#00cccc` | Secondary cyan highlight |
| `ACCENT_GLOW` | `#00ffff` (50% opacity) | Glow highlights |
| `ACCENT_DIM` | `#00ffff` (25% opacity) | Dim accents |
| `DANGER` | `#ff0000` | Stark red alerts |
| `SUCCESS` | `#00ff00` | Stark green confirmations |
| `WARNING` | `#ffff00` | Stark yellow warnings |

---

## 2. Layout, Spacing, and Scale Tokens

Consistent component spacing prevents cluttered panels and respects vertical alignment.

### A. Spacing Grid (in pixels)
| Token | Value | Applied To |
| --- | --- | --- |
| `SPACE_NONE` | `0.0` | Flat edges, tight borders |
| `SPACE_XXS` | `2.0` | Ultra-tight item spacing |
| `SPACE_XS` | `4.0` | Compact row padding, badge layout |
| `SPACE_S` | `6.0` | List row gaps, filter pills |
| `SPACE_M` | `8.0` | Component inner spacing, standard grid |
| `SPACE_L` | `10.0` | Toolbar groups, panel splits |
| `SPACE_XL` | `12.0` | Standard layout containers, modal internal gaps |
| `SPACE_XXL` | `16.0` | Page borders, side panel gutters |
| `SPACE_XXXL`| `20.0` | Heavy vertical splits, section separators |
| `SPACE_HUGE`| `24.0` | Opening screens, welcome view gutters |

### B. Typography Scale (Font sizes in points/pixels)
We use a clean, geometric sans-serif font scale (e.g. Outfit / Inter / System UI).

| Token | Size | Typical Use Case |
| --- | --- | --- |
| `FONT_SIZE_TINY` | `10` | Micro indicators, timestamp tags |
| `FONT_SIZE_SMALL` | `11` | Status bar elements, breadcrumbs, shortcuts |
| `FONT_SIZE_REGULAR`| `12` | Main UI text, sidebar list items, annotation note bodies |
| `FONT_SIZE_MEDIUM` | `14` | Editor panel headers, primary list titles |
| `FONT_SIZE_LARGE` | `16` | Modals, prominent file paths, large status toasts |
| `FONT_SIZE_HEADING`| `18` | Main view headers, document titles |
| `FONT_SIZE_TITLE` | `20` | Welcome screen title, major empty state indicators |

### C. Shape Tokens (Border Radii)
Standardized rounded corners preserve a sleek, professional interface without feeling bubbly.

| Token | Value | Applied To |
| --- | --- | --- |
| `RADIUS_NONE` | `0.0` | Screen bounds, status bars, hard panel borders |
| `RADIUS_SMALL` | `2.0` | Small tags, checkboxes, selection boundaries |
| `RADIUS_REGULAR`| `4.0` | Segmented controls, context menu items |
| `RADIUS_LARGE` | `8.0` | Main buttons, modal cards, command palette list |
| `RADIUS_ROUND` | `9999.0`| Color pills, scroll handles, rounded badges |

### D. Focus Rings and Shadow Metrics
- **Focus Indicator**: `FOCUS_RING_WIDTH = 1.5` pixels, using `ACCENT` color.
- **Divider Strength**: `DIVIDER_WIDTH = 1.0` pixels, using `BORDER` color.
- **Elevation / Shadow**: Avoid heavy dropshadows. Prefer border highlights or a flat shadow layer of width `1.0` with `Color::from_rgba(0.0, 0.0, 0.0, 0.3)`.

---

## 3. Component-State Matrix

All shared controls must present clean, visual feedback depending on their interactive states.

| Control State | Background Color | Text Color | Border Color | Extra Indication |
| --- | --- | --- | --- | --- |
| **Normal / Rest** | `BG_TERTIARY` | `TEXT_PRIMARY` | `BORDER` | None |
| **Hovered** | `BG_SURFACE` | `TEXT_PRIMARY` | `ACCENT` | Accent outline |
| **Pressed** | `BG_PRIMARY` | `ACCENT` | `ACCENT` | Inset / text color shift |
| **Focused** | `BG_TERTIARY` | `TEXT_PRIMARY` | `ACCENT` | Focus outline (`FOCUS_RING_WIDTH`) |
| **Selected** | `ACCENT_DIM` | `ACCENT_SECONDARY`| `ACCENT` | Active sidebar item/active tab |
| **Disabled** | `BG_SECONDARY` | `TEXT_MUTED` | `BORDER_SUBTLE`| Pointer events ignored |
| **Loading** | `BG_PRIMARY` | `TEXT_MUTED` | `BORDER` | Pulsing opacity / spinner |
| **Warning** | `BG_TERTIARY` | `WARNING` | `WARNING` | Amber outline |
| **Error** | `BG_TERTIARY` | `DANGER` | `DANGER` | Red outline |

---

## 4. Visual Anti-Patterns

To ensure MD Editor retains its high-quality, professional research feel, the following design treatments are **expressly forbidden**:

- ❌ **No Glassmorphism / Frosted Glass**: Do not use `backdrop-filter: blur(...)` or highly transparent overlay cards. This creates visual noise and reduces readability of dense document text.
- ❌ **No Decorative Gradients**: Gradients must never be used on panels, sidebars, or main editor surfaces. Backgrounds must be flat, solid colors.
- ❌ **No Bubbly Corners**: Do not exceed `RADIUS_LARGE` (`8.0`) on panel structures or normal cards. Standard buttons and list rows should have clean, sharp or sub-`4.0` corner values.
- ❌ **No Floating Orbs**: Do not add blurred, glowing background blobs or decorative aesthetic shapes in the margins.
- ❌ **No Marketing Layouts**: Do not design wide, empty hero spaces, landing sections, or promotional copy. The application must open immediately to either a vault selector or the active workspace.
- ❌ **No Shadow Stack Overload**: Do not use double or triple ambient shadows. Keep elevation markers flat and outline-based.

---

## 5. Migration Checklist for Existing Views

When editing any UI file under `native/src/views/`, verify the following details:
1. All raw colors (e.g. `Color::from_rgb8(...)` or inline color values) have been replaced with constants from `crate::theme::*`.
2. Static layout dimension margins (e.g. `spacing(10)`, `padding(8)`) are mapped to named spacing tokens.
3. Component font size numbers (e.g. `size(14)`) use the typography scale tokens.
4. Button styles hook into the active `AppTheme` state returned by `MdEditor::theme()`.
5. All interactive inputs (search fields, citation filters, rename popups) explicitly support focused/unfocused visuals.
