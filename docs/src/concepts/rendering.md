# Rendering

<p class="kp-lede">A game's world exists as numbers: where each object is, what it is made of, where the lights are. Rendering turns those numbers into the picture on the screen, about sixty times a second. Khora keeps several ways of drawing the same scene, from plain to rich, and draws with the one the machine can afford this moment.</p>

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
lane: each renders the same scene at a different cost/quality point. They form the
**render family**, and its agent — the engine's one `LaneAgent`, named `renderer` —
chooses the one that fits the envelope GORNA lent it.

Two principles fall out of this shape and they are worth holding onto:

- **GPU resources hide behind typed IDs.** `TextureId`, `BufferId`, `PipelineId`,
  `BindGroupId`, `SamplerId` — never a raw wgpu handle in a public API. That seam
  is exactly what lets the backend be swapped without touching a single lane.
- **Shaders are files, never strings.** Every shader is a `.wgsl` file on disk,
  composed by the `PipelineSystem` backend. Inlining shader source as a Rust constant
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
  │   ├─ event pass                 # agents woken for an event choose a row, prepare a layer
  │   ├─ OBSERVE phase
  │   │   └─ shadows layer          # encodes the shadow atlas
  │   ├─ TRANSFORM phase            # physics, audio, script layers
  │   ├─ OUTPUT phase
  │   │   ├─ renderer layer         # opaque draws, then the blended ones
  │   │   ├─ skybox layer           # the background (after renderer)
  │   │   ├─ grid / wireframe / gizmo layers   # overlays (after renderer)
  │   │   └─ ui layer               # the UI overlay
  │   └─ FINALIZE phase             # telemetry, cleanup
  │   (each family's installed layer — a lane at one level — records GPU passes
  │    into the frame graph; the scheduler runs it without calling the agent)
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

The render family offers three main-pass lanes, and its agent runs the one that fits
its envelope:

| Lane (level) | Shape |
|---|---|
| `SimpleUnlitLane` ("unlit", LowPower) | No lighting — the baseline cost floor. |
| `LitForwardLane` ("lit forward", Balanced) | PBR: every mesh shaded by every light in one pass, with shadows and image-based lighting. |
| `ForwardPlusLane` ("forward+", HighPerformance) | Tile-based light culling for many lights, with the same shading. |

Each lit lane draws the reference image up to a number of lights, and says in its
quote when the scene holds more (`Impact::Minor`), so the agent can weigh it:

- **Lit forward** shades with at most 4 directional, 16 point and 8 spot lights
  (`ShaderDefs::MAX_*`). Past those, the lights further down the frame's list are
  dropped.
- **Forward+** first cuts the target into tiles of 16 × 16 pixels and, in a compute
  pass, lists for each tile the lights that reach it, at most 128 a tile; a pixel
  is then shaded by its tile's lights only. Its light buffer holds 1024 lights. It
  needs storage buffers and compute shaders: a device without them never offers
  it. It is worth its culling pass from about twenty lights.

A switch never stalls the frame. The agent prepares the new lane's layer — its
pipelines, its buffers — beside the installed one, which keeps drawing, and the new
layer is swapped in at the next frame start. The old one is kept ready for three
epochs, so going back to it prepares nothing, and the first report after a swap
says how long the preparation took. This is what makes adaptation cheap enough to
follow the frame: GORNA lends a smaller envelope under pressure and the agent moves
to a cheaper row without a hitch.

**Shadows are a separate family** (`shadows`) — the canonical example of why the
agent/lane split exists. Its `ShadowsLane` runs in `OBSERVE`, *before* the render
family, and writes a depth shadow atlas plus a per-light shadow frame
(`ShadowFrame`, into the deck) for the lit lanes to sample. Decoupling shadow encoding from the main pass lets
the two negotiate independently: shadow quality is its own GORNA surface with three
honest tiers (high / medium / low resolution), each quoting its real VRAM cost, and
the lowest tier is always offered as the floor so the agent never returns an empty
strategy set. The tiers are the three levels of `ShadowsLane` ("low-res
shadows", "medium shadows" and "standard shadows"), and all produce the same shadow-frame contract, so the
consumer lanes are agnostic about which one ran. The subtle part is shimmer
prevention: orthographic shadow bounds are snapped to texel boundaries so shadow
edges don't crawl as the camera moves. Leave that logic alone unless you can prove
a bug.

A sun has no position, so its shadow map is fitted to the part of the view it
shades: **one cascade**, covering the first 60 world units of the camera's frustum
from its near plane (`SHADOW_CASCADE_DISTANCE`, in `khora-data/src/flow/shadow.rs`).
Over a far plane of 1000 units a 2048² map would give about one unit per texel, and
small objects would lose their shadows. A shadow further than 60 units from the
camera is not drawn. A spot light renders one perspective map over its cone and
range, a point light six, one per cube face.

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
ShadowPass        fills the shadow atlas every lit pass samples
ScenePass         opaque draws — clears colour + depth, writes depth
SkyboxPass        paints where the opaque geometry left the far plane
TransparentPass   blended draws, farthest first — tests depth, writes none
UiPass            the UI
GridPass          the editor grid
WireframePass     wireframes
GizmoPass         gizmos
```

The opaque pass, not the sky, clears the target, so a frame with no skybox family
loses only its sky.

Draw a transparent object against the sky and it composites over the sky, as it
should.

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

## A camera takes the shape of its image

A `Camera` has a field of view and near and far planes, but no aspect ratio: the
aspect belongs to the image the camera fills. Every camera projection is built
with the aspect of the colour target the frame renders into — the game window, or
the editor's viewport — read from the `ColorTargetSize` the frame publishes
(`khora_data::render::target_aspect`). Resize the window and the camera keeps its
vertical field of view and widens or narrows; it is never stretched. A run with no
colour target (headless) projects at an aspect of 1.

The first active camera is the one drawn. If its values make no image (a near plane
at 0, a far plane before the near one), the frame draws no scene and says why; no
other camera is picked in its place. A camera's scale is ignored: its view is the
inverse of its rotation and position alone.

## Image-based lighting

Light does not only come from lamps: the sky lights everything from every
direction, and shiny surfaces reflect it. **Image-based lighting** (IBL) captures
that from a picture of the surroundings. The `IblBaker` bakes it into four textures
the lit lanes sample: an environment cube (the sky itself, which the skybox draws),
an irradiance cube (the diffuse light from each direction), a prefiltered cube
(reflections, blurrier at each of its five mips for rougher surfaces) and a BRDF
table, all in linear HDR (`Rgba16Float`).

The `ibl_bake` DataSystem drives it in `PreExtract`, before any lit lane runs:

- The source is the scene's **environment map**, the `EnvironmentMap` runtime
  resource naming an equirectangular texture (an HDR file keeps the range
  reflections need). With none, the bake draws a **procedural sky**, its sun
  placed where the first shining directional light points from.
- It bakes again only when the source changes: another texture selected, or the
  texture replaced in the asset store by a hot reload. Moving the sun later does not
  bake again.
- A selected texture that has not loaded yet is waited for, up to 120 frames; then
  the procedural sky is baked and a warning names the texture.
- A new bake is published before the old one is freed, and a bake that fails keeps
  the previous one.

Until the first bake exists, and when the shadows family drew no atlas, a lit lane
binds **neutral bindings** of its own in their place: a 1×1 shadow atlas no light
points to, black environment cubes and a zero BRDF table. They add no light, so the
scene is lit by its lamps alone.

## Overlays

The editor's grid, wireframes and gizmos are families of their own, drawn after the
UI in that order (see the pass list above). The wireframe lane finds each
triangle's edges in the fragment shader, from barycentric coordinates the vertex
shader assigns, rather than with a line polygon mode, which wgpu's downlevel
baseline does not offer. Hidden edges are occluded by the scene's depth.

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
lights, give mesh entities a mesh handle and a material — and the render Flow
projects that scene every frame, the render family's agent picks the lane that fits
its GORNA envelope, and that lane draws it.
The engine's per-frame choices are visible live in the editor's GORNA panel.

## Under the hood

### The device and the window

- **One instance per backend tried.** The wgpu backend tries the preferred
  backends in order (on Windows: Vulkan, DirectX 12, then OpenGL; DirectX 11 maps to
  12) and keeps the first that yields an adapter. Each attempt makes an instance on
  that one backend only: an instance on every backend would load every driver —
  the D3D12 debug layer, the OpenGL driver — whichever is used. The surface prefers
  an sRGB format, and the `Mailbox` present mode, else `Fifo`.
- **A resize waits for the size to settle.** Dragging a window edge or a dock
  splitter asks for a new size every frame; reallocating the swapchain and depth
  textures each time would stall. A requested size is applied at once when nothing
  was applied in the last 120 ms (a maximise, a first layout); one that keeps
  changing is applied once it was asked for 3 more times unchanged, or after 10
  frames pending, settled or not. The size is clamped to what the device can hold.
- **The frame waits for the last submission before it acquires.** Before taking the
  next swapchain image, the render system waits until the GPU processed every
  submission so far, from every thread: acquiring sooner trips Vulkan's
  `VUID-vkAcquireNextImageKHR-semaphore-01286`.

### Group 3 of the lit lanes

Group 3 holds what lights a surface. Both lit lanes share the shadow part of it
(`khora_core::renderer::api::shadow::bindings`) and the four bindings of the IBL
block, which sits at a base each lane picks:

| Binding | Lit forward (`LayoutKey::Lighting`) | Forward+ (its own layout) |
|---|---|---|
| 0 | the lighting uniforms | the lights (storage) |
| 1, 2, 3 | the 2D shadow atlas, the comparison sampler, the cube atlas | the same |
| 4 | IBL irradiance cube | the per-tile light indices |
| 5 | IBL prefiltered cube | the light grid (offset and count per tile) |
| 6 | IBL BRDF table | the tile info |
| 7 | IBL sampler | the lights' shadow view-projections |
| 8 – 11 | — | the IBL block |

### Mirrored draws

A transform with a negative determinant (an odd number of negative scales) turns a
mesh's counter-clockwise faces clockwise. A single-sided material then culls its
**front** faces instead of its back ones, so the faces seen from outside are still
drawn; every pipeline keeps a counter-clockwise front face, and the cull mode alone
tells the pipelines apart in their cache. A custom shader reading `front_facing`
sees `false` on the visible faces of a mirrored draw.

### Materials on the GPU

- **A texture's colour space comes from its slot**, not from its file: one PNG is a
  colour map or a data map depending on where a material uses it. Colour maps are
  sampled as sRGB, which the GPU turns to linear values on read; data maps are read
  as they are, since converting them would break the lighting maths. Float textures
  (`Rgba16Float`, `Rgba32Float`) are linear whatever the slot.

  | Slot | Read as |
  |---|---|
  | base colour, emissive, specular colour | sRGB |
  | normal, metallic-roughness, occlusion, specular strength | linear |

- **One sampler for every material.** The material sampler is made once, the first
  time the device accepts it, and shared. Evicting a material frees its uniform
  buffer, its textures, their views and its bind group, never the sampler. How
  residents are made and evicted is in [Residency](./residency.md).

## Next steps

- [How-to: materials and lighting](../how-to/materials-and-lighting.md) — set up a
  camera, lights, and PBR materials.
- [The frame](./the-frame.md) — the fixed-timestep simulation clock and how render
  interpolation rides on it.
- [Assets](./assets.md) — how meshes, textures, and materials reach the renderer.
- [Glossary](../reference/glossary.md) — RenderSystem, strategy, frame graph, Flow.
