# UI

In-engine UI as ECS entities, laid out behind a single trait and rasterized through
the same GPU device as everything else. This page explains *why* UI is retained-mode
ECS with a swappable layout engine, and how it plugs into the frame. It is an
explanation, not a recipe — for the steps to build a panel, see
[How-to: build a UI](../how-to/build-a-ui.md).

---

## The contract is one trait

UI layout is the **`LayoutSystem`** trait in `khora-core` (`ui/`). It is the only
thing the rest of the engine knows about layout: hand it a node tree and available
space, get back computed positions and sizes. The *renderer* is a separate concern —
a dedicated UI lane rasterizes the laid-out nodes. Splitting layout from rendering
behind a trait is what lets a different layout engine drop in without touching the
lanes that draw.

The default implementation is a Taffy-backed layout system in `khora-infra`
(`ui/taffy/`), giving flex and grid layout. Khora maps its own UI components onto
Taffy's style model, runs the layout, and reads back per-node geometry. Taffy types
never leak into components — the mapping happens inside the layout lane — so the
backend stays swappable behind the trait, the same boundary discipline the rest of
the engine follows.

## UI is just entities

There is no separate UI tree and no separate registry. **UI is entities with UI
components**, living in the same ECS as everything else, in the `UI`
[semantic domain](../reference/glossary.md). A panel is an entity with a layout node
and a visual style; an image element adds an image component; hierarchy uses the
same parent/child relationship as scene hierarchy — no bespoke UI parent type.

The component vocabulary is small and structural: a **layout node** carries sizing,
min/max, padding, margin, and flex properties; a **style** carries background and
border colour, border width and radius, and an optional texture; an **image**
component references a texture to draw. They are screen-space by design — that is
the whole reason UI uses its own layout node rather than the world-space transform
the renderer uses. The concrete builder API lives in
[How-to: build a UI](../how-to/build-a-ui.md).

## How it plugs into a frame

UI follows the same CLAD descent as every other domain (`Control → Agent → Lane →
Data`), as two lanes owned by the **`UiAgent`**:

```
ECS (layout nodes, styles, images, hierarchy)
  ↓ UI Flow projects a UiScene into the LaneBus
layout lane   — runs the LayoutSystem, produces pre-laid-out nodes
  ↓
UI render lane — rasterizes the scene, compositing over what the renderer drew
```

The data layer's UI **[Flow](../reference/glossary.md)** projects the UI domain into
a scene View on the LaneBus; the layout lane computes geometry through the
`LayoutSystem` trait; the render lane draws the result in the frame's `OUTPUT`
phase, loading the existing colour target so it composites over the scene the
renderer already produced. Two passes, same shape as the scene render path: a
compute-style pass followed by a draw pass.

Notably, there is no separate "UI renderer" — the UI render lane shares the one GPU
device with everything else, reached through the same typed-ID abstraction. Text
rendering uses a glyph cache and atlas that live in the backend because they depend
on that device.

## Editor UI versus game UI

Today the `UiAgent` runs in the editor. Its negotiation surface is minimal — one
strategy, no real GORNA pressure yet — because editor UI complexity hasn't demanded
density tiers (full / simplified / hidden chrome) that the design leaves room for.
In-game (play-mode) UI is on the roadmap; the path is mostly a matter of which modes
the agent is allowed to run in, plus deciding the input model. The retained-mode,
ECS-driven shape is the same either way.

## Next steps

- [How-to: build a UI](../how-to/build-a-ui.md) — spawn a panel and child elements
  with the real component API.
- [Data and the ECS](./ecs.md) — how UI components are stored, domained, and
  projected.
- [Rendering](./rendering.md) — the scene pass the UI overlay composites over.
- [Glossary](../reference/glossary.md) — LayoutSystem, semantic domain, Flow, lane.
