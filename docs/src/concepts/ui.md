# UI

<p class="kp-lede">The score in a corner, a health bar, a menu: a game's interface is drawn flat over the world, in the pixels of the screen. In Khora each panel, label and picture of that interface is an object of the game like any other, and the engine draws them over the scene every frame.</p>

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
never leak into components — the mapping happens inside the Taffy backend — so the
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
Data`), through **one** lane, `UiLane`, the only lane of the `ui` family (run, like
every family, by the engine's one `LaneAgent`):

```
ECS (layout nodes, styles, images, hierarchy)
  ↓ UiFlow projects a UiScene into the LaneBus
UI render lane — rasterizes the scene, compositing over what the renderer drew
```

The data layer's UI **[Flow](../reference/glossary.md)** projects the UI domain
into a scene View on the LaneBus, and `UiLane` draws it in the frame's
`OUTPUT` phase, loading the existing colour target so it composites over the scene
the renderer already produced.

> **Layout is not wired yet.** The `LayoutSystem` trait exists in `khora-core` and
> the Taffy backend implements it, but **nothing calls `compute_layouts`**:
> `UiFlow` reads the `UiTransform` an author already placed. Until that gap
> closes, UI positions are authored, not computed — the trait and the backend are
> the shape the engine intends, not a path a frame currently takes.

Notably, there is no separate "UI renderer" — the UI render lane shares the one GPU
device with everything else, reached through the same typed-ID abstraction.

## Text

Text goes through three steps, each behind a trait so a backend can be swapped:

- a **shaper** (`TextShaper`; Parley by default) turns a string into glyphs placed
  on lines, once per change of the text (`ui_text_layout`);
- a **rasteriser** (`GlyphRasterizer`; swash by default) draws each glyph into a
  small image, once;
- `glyphs_on_gpu` places those images in the **glyph atlas** on the GPU, and the UI
  lane draws each glyph as a quad sampling its rectangle.

A font file is read by one rule both backends share: its **first face**. A font
collection (`.ttc`) holds several faces; the shaper would otherwise pick the one
nearest the weight it asks for while the rasteriser drew the first, and the text
would be laid out in one face and drawn in another.

**Glyphs land on whole pixels.** A glyph image is keyed by its font, its glyph, its
size rounded to a quarter of a pixel, and where its pen falls within a pixel,
rounded to one of four quarter-pixel bins. The image of a bin already holds that
fraction, so the lane draws it from a whole pixel: the text's origin and each
line's baseline are rounded, and each texel of the image lands on one pixel of the
screen. Text stays sharp wherever it is placed, and the same glyph at pens a whole
number of pixels apart shares one image.

## Editor UI versus game UI

The `ui` family is one of the families the engine registers, so it runs for every SDK application, not only the editor. Its negotiation surface is minimal — one
level, no real GORNA pressure yet — because editor UI complexity hasn't demanded
density tiers (full / simplified / hidden chrome) that the design leaves room for.
In-game (play-mode) UI is on the roadmap; the family already runs whether the game is edited
or played, and an app can switch it off through the `Activation`; what is left is
deciding the input model. The retained-mode,
ECS-driven shape is the same either way.

## Next steps

- [How-to: build a UI](../how-to/build-a-ui.md) — spawn a panel and child elements
  with the real component API.
- [Data and the ECS](./ecs.md) — how UI components are stored, domained, and
  projected.
- [Rendering](./rendering.md) — the scene pass the UI overlay composites over.
- [Glossary](../reference/glossary.md) — LayoutSystem, semantic domain, Flow, lane.
