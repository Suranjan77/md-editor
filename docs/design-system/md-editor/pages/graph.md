# Page override — Knowledge graph

The graph view has its own widget set today (`views/graph/widgets.rs`, ~150 literal sizes and
colours). It moves onto the shared components; only what is genuinely graph-specific stays.

## Node and edge encoding

Colour is never the only channel (MASTER §2.3):

| Kind | Shape | Dark fill | Light fill |
| :--- | :--- | :--- | :--- |
| Markdown note | filled circle | `accent` | `accent` |
| PDF | rounded square | `status.info` | `status.info` |
| Missing target | hollow circle, dashed ring | `fg.tertiary` | `fg.tertiary` |
| Selected | + 2px `focus.ring` halo | — | — |
| Pinned | + small pin glyph | — | — |

Edges: `border.control` at 40%; wikilinks solid, PDF-annotation links dashed. Hover dims
non-neighbours to 25% (above the 0.2 floor in the skill's `opacity-threshold`).

## Chrome

- Toolbar uses the shared `Toolbar`/`Segmented` (Global · Local, depth 1 · 2), `TextField`
  (search), and filter `Chip`s.
- Inspector is a standard right `Panel` (same width and header as Backlinks).
- Legend is a `Card` in the lower-left with shape + label for every kind.
- Labels: `caption` size, `fg.secondary`, on a `bg.canvas` @ 70% backing for legibility.

## Motion and alternatives

- Reduced motion: the simulation settles off-screen and the result is shown directly.
- Every drag has a non-drag path: pin/unpin and "centre on" from the inspector; arrow keys
  move selection between neighbours; `+`/`−`/`0` zoom and fit.
