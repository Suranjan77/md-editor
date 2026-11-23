# Knowledge Graph

The knowledge graph is a derived view of the vault's links: notes, PDFs, and the targets
links point at, drawn as a force-directed graph. It never writes to the vault. Users open it
with `Ctrl+G`, the toolbar, or *Open Knowledge Graph* in the palette; see the
[User Guide](User-Guide-and-Feature-Manual.md#knowledge-graph) for what it offers.

| Layer | Where | Owns |
| :--- | :--- | :--- |
| Snapshot | `core/src/vault/graph.rs` → `get_graph_snapshot` | Building `GraphSnapshot` (nodes + edges) from the index and the PDF sidecar DB |
| Data types | `core/src/types.rs` | `GraphSnapshot`, `GraphNode`/`GraphNodeKind`, `GraphEdge`/`GraphEdgeKind` |
| State | `native/src/graph_state.rs` | `GraphState`: snapshot, scope, filters, selection, pins, view transform, refresh generations, insights |
| Simulation | `native/src/graph_state/physics.rs` | Barnes–Hut quadtree, `PhysicsState` (prepare / tick / nudge, settle budgets) |
| Initial layout | `native/src/graph_state/layout.rs` | Adjacency, connected components, island packing, seeded positions |
| Messages | `native/src/app/graph.rs` → `update_graph` | Every `Graph*` message |
| GPU layer | `native/src/views/gpu_graph.rs`, `native/shaders/graph.wgsl` | Instanced nodes and edges; all pointer interaction |
| Overlay and chrome | `native/src/views/graph.rs`, `views/graph/` | Labels and hover card (canvas), toolbar, inspector, legend, empty states |

---

## 1. The Snapshot (core)

`get_graph_snapshot(&AppState)` returns a deterministic `GraphSnapshot`:

- **Nodes** — every indexed Markdown file and every PDF on disk, connected or not, plus a
  `Missing` node for each link target that does not resolve. A node's id is its
  vault-relative path with `/` separators.
- **Edges** — `WikiLink` edges from the `FileIndex`'s outgoing map, and `PdfAnnotation`
  edges from PDF highlights linked to notes, aggregated per document/note pair.

The vault-root and index locks are held only long enough to clone what is needed, under the
same publication barrier as vault switching, so a snapshot can never mix one vault's files
with another's annotations. Directory walking and database reads happen afterwards, off the
UI thread.

## 2. Refresh

`GraphState` tracks staleness with a generation counter rather than rebuilding on every
change:

1. Anything that can change links — a save, a create/rename/delete, a watcher event —
   calls `MdEditor::invalidate_graph()` → `GraphState::mark_dirty()`.
2. While the graph is visible and stale, `app/subscription.rs` ticks `GraphRefresh` every
   200ms; `begin_refresh()` hands out a generation and the snapshot is built in a `Task`.
3. `GraphSnapshotLoaded(generation, result)` is applied only if the generation is still
   current. A failed generation is held until the next invalidation or an explicit retry,
   so a persistent database error cannot become a tight retry loop.

Unsaved edits do not appear until the note is saved (autosave makes that quick).

## 3. Layout and Simulation

A new snapshot is laid out in two steps:

- **Seeding** (`layout.rs`) — nodes are grouped into connected components ("islands"),
  each island is placed with a golden-angle spiral, and islands are packed with padding so
  disconnected groups do not start on top of each other. Positions are seeded from a hash of
  the path, so the same vault lays out the same way every time.
- **Settling** (`physics.rs`) — a force simulation: springs along edges and Barnes–Hut
  repulsion (`THETA = 0.75`) through a quadtree rebuilt each tick. `prepare()` rebuilds the
  structure when the node or edge set changes; `nudge()` wakes the simulation after a drag
  without rebuilding. It stops itself after `SETTLE_BUDGET` ticks (or `NUDGE_BUDGET` after a
  nudge), and the 33ms tick subscription is armed only while it runs.

Pinned nodes are excluded from integration but still exert forces. Dragged positions last
for the session only.

## 4. Rendering and Input

The view is a `stack` of two layers sharing one transform:

- **`GpuGraphProgram`** (`gpu_graph.rs`) — an iced `shader` widget that draws edges then
  nodes as instanced quads, generated in the vertex shader. It uses no storage buffers or
  compute passes, which keeps it inside iced's default device limits. It owns pan, zoom,
  drag, hover, selection and fit, and publishes the result as `GraphSetView`,
  `GraphHovered`, `GraphNodeMoved` and `GraphNodeSelected`.
- **`GraphLabels`** (`views/graph/labels.rs`) — a canvas over it that draws node labels,
  thinned by a degree cutoff once the graph is dense, and the hover card.

`views/graph/geometry.rs` holds the shared transform (`ViewTransform`, world ⇄ screen) and
node sizing, so both layers agree on where a node is. Zoom is clamped to
`MIN_ZOOM..=MAX_ZOOM` (0.002–6.0).

## 5. Filtering and Insights

`GraphState::filtered()` caches the scope-, filter- and query-reduced graph; both render
layers share that one allocation per rebuild.

- **Scope** — global, or local around the active note/PDF at depth 1 or 2.
- **Query** — narrows to matching nodes while keeping their direct neighbours for context.
- **Filters** — PDFs, missing targets, unconnected nodes. None of them touch vault files.
- **Insights** — `stats()` (notes, PDFs, missing targets, connections, unconnected nodes,
  components), `hubs()`, `unconnected()`,
  `missing()`, and `related(path)`: notes sharing neighbours with the selection but not
  directly linked to it, ranked by Adamic–Adar score (shared neighbours weighted by
  `1 / ln(1 + degree)`, so a shared rare link counts for more than a shared hub).

## 6. Tests

`native/src/graph_state/tests.rs` covers filtering, scope, insights and the simulation;
`views/graph.rs` tests culling, hit testing and zoom anchoring; core snapshot behaviour is
tested in `core/src/vault/tests.rs`.
