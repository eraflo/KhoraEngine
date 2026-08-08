# Rendering

The subsystem that turns ECS components into pixels. This page explains *why* the
renderer is shaped as a family of strategies behind a single trait, how it plugs
into the CLAD descent and GORNA negotiation, and how it stays alive when the GPU
does not. It is an explanation, not a recipe — for the steps to set up a camera, a
light, or a material, see [How-to: materials and lighting](../how-to/materials-and-lighting.md);
for the exact types, the rustdoc on `khora_core::renderer`.

---

## Why a family of strategies

A modern engine must render across hardware that varies by orders of magnitude —
from a handheld to a workstation. Picking a single render path at compile time
leaves performance on the table everywhere except the developer's machine.

So Khora's renderer is not one pipeline; it is a *family* of them behind one
abstraction. The rendering surface is the **`RenderSystem`** trait in `khora-core`
(`renderer/traits/render_system.rs`) — the only thing the rest of the engine knows
about. Concrete GPU work lives in a backend under `khora-infra`; the default is
wgpu. Above that trait, a **[strategy](../reference/glossary.md)** is a self-contained
lane: each renders the same scene at a different cost/quality point, and the
**`RenderAgent`** picks one per frame based on the budget GORNA approved.

Two principles fall out of this shape and they are worth holding onto:

- **GPU resources hide behind typed IDs.** `TextureId`, `BufferId`, `PipelineId`,
  `BindGroupId`, `SamplerId` — never a raw wgpu handle in a public API. That seam
  is exactly what lets the backend be swapped without touching a single lane.
- **Shaders are files, never strings.** Every shader is a `.wgsl` file on disk,
  composed through the shader registry. Inlining shader source as a Rust constant
  is forbidden, because files are editable, reviewable, and hot-reloadable.

## The frame lifecycle

Rendering is one slice of the per-frame descent (`Control → Agent → Lane → Data`).
The engine acquires the swapchain once, runs the scheduler, and presents once:

```
EngineCore::tick
  ├─ run_app_update                 # user logic + Substrate DataSystems
  │
  ├─ begin_render_frame             # RenderSystem::begin_frame
  │   └─ acquire swapchain texture; publish color/depth targets
  │
  ├─ run_scheduler
  │   ├─ Substrate Pass             # Flows project Views into the LaneBus
  │   ├─ OBSERVE phase
  │   │   └─ ShadowAgent.execute    # encodes the shadow atlas
  │   ├─ TRANSFORM phase            # physics, audio, scripting
  │   ├─ OUTPUT phase
  │   │   ├─ RenderAgent.execute    # opaque draws, then the blended ones
  │   │   ├─ SkyboxAgent.execute    # the background
  │   │   ├─ OverlayAgent.execute   # grid, wireframe, gizmos
  │   │   └─ UiAgent.execute        # the UI overlay
  │   └─ FINALIZE phase             # telemetry, cleanup
  │   (agents record GPU passes into the frame graph)
  │
  ├─ end_render_frame               # submit the frame graph, then present
  │
  └─ run_maintenance                # drain outputs, compact the ECS
```

One acquire, one present, per frame. Lanes never encode into a shared command
buffer; they record passes — each declaring the resources it reads and writes —
into a **frame graph**, and the engine drains and submits that graph after the
scheduler returns. This stays deliberately simple: it is a topologically-ordered
pass list (a shadow-atlas *write* always precedes a shadow-atlas *read*, whatever
order the lanes ran in), not a full render-graph framework with transient resource
pooling or aliasing. The handful of passes per frame today does not warrant that
complexity; the design revisits it only when the per-frame pass count grows.

Because the renderer reads Views the data layer projected — never the World
directly — it sees a consistent snapshot. The producer of those Views is the render
**[Flow](../reference/glossary.md)**, the only legitimate source of the render and
shadow Views the lanes consume from the LaneBus.

## Strategies and shadows

The `RenderAgent` selects one main-pass strategy per frame:

| Strategy | Shape |
|---|---|
| Unlit | No lighting — the baseline cost floor. |
| Lit forward | PBR with per-light passes and shadow sampling. |
| Forward+ | Tile-based light culling for many lights. |
| Standard PBR | The physically-based material model the lit paths shade through. |

The pipelines for every strategy are compiled at boot, so switching between them
is a bind-group flip rather than a stall. This is what makes per-frame negotiation
cheap enough to do every frame: GORNA can ask for a cheaper strategy under pressure
and the agent honours it without rebuilding anything.

**Shadows are a separate agent** — the canonical example of why the
agent/lane split exists. The `ShadowAgent` runs in `OBSERVE`, *before* the
`RenderAgent`, and publishes a depth shadow atlas plus a per-light shadow frame for
the lit consumer lanes to sample. Decoupling shadow encoding from the main pass lets
the two negotiate independently: shadow quality is its own GORNA surface with three
honest tiers (high / medium / low resolution), each quoting its real VRAM cost, and
the lowest tier is always offered as the floor so the agent never returns an empty
strategy set. All three tiers produce the same shadow-frame contract, so the
consumer lanes are agnostic about which one ran. The subtle part is shimmer
prevention: orthographic shadow bounds are snapped to texel boundaries so shadow
edges don't crawl as the camera moves. Leave that logic alone unless you can prove
a bug.

## Transparency, and the order it needs

A material with `AlphaMode::Blend` joins a second batch. Everything about how it
is drawn follows from one constraint: **a blended surface must not write depth.**
If it did, two panes of glass could not composite — the nearer one would reject
the farther one's fragments before they were ever mixed in.

Two consequences a game developer can rely on:

**Blended objects are sorted back-to-front, per object.** The sort key is the
squared distance from the camera to the model's origin. That is exact for
separated objects and approximate for intersecting or concave ones — two glass
spheres that overlap in depth may composite in the wrong order, and no
per-object sort can fix that. It is the standard trade: per-fragment ordering
costs far more than the artefact does.

**The sky is drawn between the two batches.** Because blended surfaces leave the
depth buffer at the far plane, and the skybox paints exactly the pixels left
there, a sky drawn afterwards would repaint the glass. The frame graph folds the
passes in this order:

```
ScenePass         opaque draws — clears colour + depth, writes depth
SkyboxPass        paints where the opaque geometry left the far plane
TransparentPass   blended draws, farthest first — tests depth, writes none
OverlayPass       grid, wireframe, gizmos
UiPass            the UI
```

Draw a transparent object against the sky and it composites over the sky, as it
should. Before the passes were split it disappeared there — while the half of it
overlapping a wall stayed, because the wall had written depth.

**For glass, set `double_sided: true`.** Otherwise back faces are culled and a
sphere you can see into looks hollow.

## Render interpolation

The simulation advances in whole fixed steps while rendering happens once per
variable-rate frame, so most frames fall *between* two sim steps. Drawing the raw
simulated pose would judder. To stay smooth, the render Flow blends each simulated
body's pose toward its current one by the interpolation factor the scheduler
published for the frame.

The "previous" pose is captured each step into a render-only store — deliberately
*not* an ECS component, because interpolation is representation, not game state
("adapt the HOW, never the WHAT"). Keeping it out of component space means it never
shows up in the inspector and is never serialized. The cost is **one fixed-step of
interpolation latency** — the standard trade for jitter-free motion at any frame
rate. The full mechanism, including why the simulation clock is fixed-step, lives
in [The frame](./the-frame.md).

## Device-loss resilience

A GPU hiccup must never panic the frame loop. The backend classifies every
surface-acquire outcome and every device-health condition into an explicit action:

- **Transient surface conditions self-heal.** A lost or outdated swapchain at a
  valid size is reconfigured and the acquire retried **once** in-frame; a minimized
  or not-yet-ready window simply skips the frame quietly and retries next frame —
  no error spam, no unbounded spin.
- **Device loss and out-of-memory are reported, not hidden.** These can't recover
  in place, so they are observed through the backend's error callbacks as sticky
  flags. Before any submission the render system checks them and, if set, returns a
  **fatal but non-panicking** error so the host can tear down cleanly.

This is honest about its limits: full device recreation is not yet implemented.
What is guaranteed is that the loop degrades to a clean fatal error rather than
crashing mid-frame, and that transient conditions recover on their own.

## For game developers, the model is declarative

You do not call render functions. You describe a scene — spawn a camera, spawn
lights, give mesh entities a mesh handle and a material — and the `RenderAgent`
extracts that scene every frame, picks the strategy GORNA approved, and draws it.
The engine's per-frame choices are visible live in the editor's GORNA panel.

## Next steps

- [How-to: materials and lighting](../how-to/materials-and-lighting.md) — set up a
  camera, lights, and PBR materials.
- [The frame](./the-frame.md) — the fixed-timestep simulation clock and how render
  interpolation rides on it.
- [Assets](./assets.md) — how meshes, textures, and materials reach the renderer.
- [Glossary](../reference/glossary.md) — RenderSystem, strategy, frame graph, Flow.
