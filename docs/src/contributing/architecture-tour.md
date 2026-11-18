# Architecture tour

<p class="kp-lede">A city is easier to learn walking one street after another than reading the whole map at once. This page is that walk through Khora: the order in which to read the concept pages, so each one builds on the last, from why the engine exists down to the part you are about to change.</p>

The [Concepts](../concepts/index.md) section explains every idea in Khora. This
page adds no architecture of its own; it points into those pages in the order
that builds the picture fastest. Each line says **what you'll get** from the page.

## The core mental model

1. **[The big idea — SAA](../concepts/saa.md)**
   *What you'll get:* why an engine would negotiate with itself — the Symbiotic
   Adaptive Architecture, where each subsystem is offered a share of the frame
   and chooses how to spend it, rather than running one fixed code path.

2. **[Architecture — CLAD](../concepts/clad.md)**
   *What you'll get:* the `Control → Agent → Lane → Data` descent that names the
   command path of a frame, and the downward dependency direction every crate
   obeys.

3. **[The frame](../concepts/the-frame.md)**
   *What you'll get:* the per-frame loop end to end — the frame boundary, the
   phases and their waves, the background, fixed timestep and render
   interpolation, and where the CLAD descent happens each tick. This is the spine
   everything else hangs off.

## The adaptive machinery

4. **[Data and the ECS — CRPECS](../concepts/ecs.md)**
   *What you'll get:* how entities and components are stored, queried, and laid out
   — the data layer at the bottom of the descent.

5. **[Agents and Lanes](../concepts/agents-and-lanes.md)**
   *What you'll get:* the split between the strategist that *chooses* (the agent)
   and the executors that *run* (the lanes); why a lane runs as a layer at its
   family's place without the agent; the bus and the deck a lane reads and writes.

6. **[Families and levels](../concepts/families-and-levels.md)**
   *What you'll get:* what a family is made of — its outputs, its lanes, their
   levels, the one `register_agent!` line — and how levels become the priced rows
   an agent chooses among.

7. **[GORNA](../concepts/gorna.md)**, then **[the protocol](../concepts/gorna-protocol.md)**
   *What you'll get:* how the frame's time is shared — the offers, the envelopes
   lent at a frame start, the reports after every run, the epochs and their
   triggers. This is the heart of SAA in practice.

8. **[The arbiter](../concepts/arbiter.md)**
   *What you'll get:* how the DCC decides an epoch — the cost book, the proposers,
   the shield every plan must pass, the frame governor.

## Then: the subsystem you care about

With the spine in place, read the one or two subsystems your change touches. Each
is a `khora-core` trait with a default backend in `khora-infra`, and a family in
`khora-lanes`:

- [Rendering](../concepts/rendering.md) · [Physics](../concepts/physics.md) ·
  [Audio](../concepts/audio.md) · [UI](../concepts/ui.md) ·
  [Scripting](../concepts/scripting/index.md)
- [Assets](../concepts/assets.md) · [Residency](../concepts/residency.md) ·
  [Trust](../concepts/trust.md) · [Serialization](../concepts/serialization.md) ·
  [Telemetry](../concepts/telemetry.md)

If your work is about data layout adaptation, also read
[AGDF](../concepts/agdf.md) — how the data layer changes its representation at
runtime without ever changing meaning ("adapt the HOW, never the WHAT").

## Navigating the code itself

Three facts make the codebase legible once you start reading it:

- **Dependencies flow downward, tier by tier**: the floor (`khora-core`,
  `khora-macros`), the crates on it (`khora-data`, `khora-control`,
  `khora-script`, `khora-telemetry`, `khora-infra`, `khora-tool-ui`), the middle
  (`khora-io`, `khora-lanes`), the strategists (`khora-agents`), the façade
  (`khora-sdk`), the apps. There are no cycles. The
  [Crate map](../reference/crates.md) says what each crate holds and may use,
  which tells you, for any symbol, the crate it must be in.
- **A subsystem is a family.** Its folder in `khora-lanes/src/<family>/` holds
  its `LaneFamily`, its lanes and its `registration.rs`; its agent is the engine's
  one `LaneAgent`. Read the folder, not an agent type.
- **Query the codegraph before grepping.** The repo ships a codegraph index of
  every symbol and edge. Use it to jump straight to a definition, find callers, or
  see what a change would impact, instead of text-searching the tree.

## Backends a concurrent wave reaches

Lanes prepare and record in concurrent waves, so a backend a lane reaches — the
graphics device, the pipeline system, an atlas, a text backend — is called from
several threads at once. None of them may deadlock, and none does **by
construction**: every lock is either a **leaf**, held only to look an entry up,
insert or remove it and never across a call out, or sits above leaves in one order
that only descends. Nothing calls back up while it holds a lock.

| Backend | Its lock | Held across |
|---|---|---|
| `WgpuDevice` | one `Registry` (the id tables) and one surface record, both leaves | nothing: wgpu's device and queue are thread-safe |
| `WgpuPipelineSystem` | one lock over the composer and the caches | calls to the device, below it |
| `GlyphAtlas` | one lock over the atlases and the slots | calls to the device that upload into or replace its own textures, never the rasteriser |
| `TextShaper`, `GlyphRasterizer` (Parley, swash) | one leaf each, around their fonts and contexts | nothing |
| `DefaultMixBus` | one leaf around the sample queue | nothing |

So the order is *pipeline system or glyph atlas → device*, and nothing else nests.
A new backend states its order in its trait's docs (as `GraphicsDevice` and
`PipelineSystem` do, in `khora-core`), and a concurrency test on the real
implementation guards it: `khora-infra/src/graphics/wgpu/device/tests/concurrency.rs`
drives creation, writes, destruction, pipelines and submissions from many threads
under a watchdog.

Two more rules of the wgpu backend a change runs into:

- **Every flag set that crosses into wgpu is bridged row by row**
  (`khora-infra/src/graphics/wgpu/flag_bridge.rs`): `BufferUsage`, `TextureUsage`,
  `ShaderStageFlags` and `ColorWrites` map flag by flag through `bridge_flags!`, and
  its test `every_declared_flag_is_bridged` fails on a Khora flag no row maps. A new
  flag gets its row in the same change.
- **GPU tests take the GPU one at a time.** A test that makes a device holds
  `exclusive_gpu()` while its device lives, and asks for an adapter on the platform's
  own backend only (Vulkan; Metal on macOS; a software adapter, then WARP, without
  one). Several devices driven at once crash the Vulkan validation layer, and an
  instance on every backend loads drivers no test uses; one device per process holds,
  as in the engine, however many threads drive it.

---

Got the map? Now learn the rules every change must follow:
[Conventions](./conventions.md).
