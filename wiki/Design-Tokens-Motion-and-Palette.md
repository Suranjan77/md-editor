# Design Tokens, Motion & Command Palette

MD Editor achieves visual polish and low latency through a strict design token system, a cohesive motion vocabulary with 0% idle CPU overhead, and a contextual fuzzy matching engine.

---

## 1. Design Token System (`native/src/theme.rs`)

UI chrome elements (sidebar, toolbar, panels, modals, dialogs) do not use arbitrary pixel values or hardcoded hex colors. Every visual metric is declared in [`theme.rs`](file:///home/sur/repo/md-editor/native/src/theme.rs).

### The Six-Step Type Scale

Chrome typography adheres strictly to six sizes:

| Token | Size | Intended UI Use |
| :--- | :--- | :--- |
| `TEXT_XS` | 10.0 px | Fine print, badges, count pills, gutter line numbers |
| `TEXT_SM` | 12.0 px | Secondary text, captions, metadata, tree folder labels |
| `TEXT_BASE` | 14.0 px | Default UI controls, button labels, input fields, palette rows |
| `TEXT_MD` | 16.0 px | Emphasized UI text, group section headers |
| `TEXT_LG` | 18.0 px | Panel titles, modal headers, dialog titles |
| `TEXT_DISPLAY`| 42.0 px | Welcome screen wordmark (deliberately the only display size) |

*Note: Document typography inside Markdown notes (headings H1–H6, inline code, body prose) is managed separately by the markdown canvas renderer to adhere to typographic standards.*

### The Seven-Step Spacing Grid

Gaps, margins, and padding follow a 2px geometric grid:

```
SPACE_0 (0px)  ─── Flush / hairline
SPACE_1 (2px)  ─── Micro-offset between tightly grouped items
SPACE_2 (4px)  ─── Icon-to-label spacing within buttons
SPACE_3 (8px)  ─── Default gap between siblings; standard control padding
SPACE_4 (12px) ─── Between grouped rows; comfortable panel padding
SPACE_5 (16px) ─── Section separation within cards and panels
SPACE_6 (24px) ─── Major region margins and outer window gutters
```

### Corner Radii

- **`RADIUS_SM` (3.0 px)**: Badges, pills, inline tags.
- **`RADIUS_MD` (6.0 px)**: Buttons, text input boxes, dropdown rows.
- **`RADIUS_LG` (10.0 px)**: Panels, modal containers, cards.

---

## 2. Motion Subsystem (`native/src/motion.rs`)

Abrupt layout snapping creates visual jarring, while excessive or slow animations induce fatigue. MD Editor standardizes on a minimal motion vocabulary:

### The Single Easing Curve
```rust
pub const EASE: Easing = Easing::EaseOutQuad;
```
A decelerating quadratic curve mimics natural physical settlement (objects slowing to rest), which matches panels sliding open or toasts landing.

### Standard Durations

```
FAST    (120 ms) ─── Micro-transitions: toasts arriving, badge updates
OVERLAY (140 ms) ─── Modals, Command Palette popup
PANEL   (180 ms) ─── Sidebar, Table of Contents, Backlinks panel slides
```

### Zero Idle CPU Architecture

Desktop applications should not consume battery or CPU cycles when idle. MD Editor guarantees **0% idle CPU usage**:

```mermaid
flowchart LR
    Event[UI Action: Toggle Panel] --> SetAnim[Set motion animation target]
    SetAnim --> ArmSub[iced::window::frames() armed]
    ArmSub --> DrawLoop[Draw intermediate animated frames at display refresh rate]
    DrawLoop --> CheckDone{Have all animations settled?}
    CheckDone -->|No| DrawLoop
    CheckDone -->|Yes| DisarmSub[iced::Subscription::none() returned]
    DisarmSub --> Sleep([0% CPU Idle Sleep State])
```

The per-frame redraw subscription is armed **strictly** when `motion.is_animating()` is `true`. As soon as the animation duration completes, the subscription returns `Subscription::none()`, returning the application to an idle sleep state until the next user event.

---

## 3. Fuzzy Matcher & Command Palette (`fuzzy.rs`, `views/command_palette.rs`)

Pressing `Ctrl+P` opens the Command Palette, enabling rapid keyboard navigation without reaching for a mouse.

### Ranking Heuristics (`fuzzy.rs`)

The fuzzy matching algorithm evaluates query characters sequentially against target strings:

```rust
pub fn score(haystack: &str, query: &str) -> Option<i32>
```

Scoring rules:
1. **In-Order Requirement**: Query characters must appear in order, but need not be adjacent (e.g., typing `tconf` matches `Tracker Configuration`).
2. **Word Boundary Bonus (+18 points)**: Matching the start of a word (after whitespace, slashes, underscores, hyphens, or parentheses) yields significant bonus points.
3. **CamelCase Bonus (+10 points)**: An uppercase letter inside a word counts as a word start, allowing users to type initials (e.g., `sv` ranks `Split View` at the top).
4. **Consecutive Run Multiplier**: Matching consecutive characters yields progressive bonus points ($+10 + 6 \times \text{run\_length}$), prioritizing exact substrings.
5. **Filename over Directory (+15 points)**: Matches occurring within the filename itself outrank matches in parent folder paths.
6. **Distance Penalty**: Gaps between matched characters decrement the score, favoring compact matches.

### Unified Interleaved Registry

Commands and vault files are ranked on the same numeric scale and interleaved by score:
- **Top Command**: Execute immediately by pressing `Enter`.
- **Top Note**: Opens the Markdown note in the editor.
- **Top PDF**: Opens the document in the PDF viewer.
- **Navigation**: Arrow keys (`Up` / `Down`) cycle through results; `Escape` dismisses the palette.
