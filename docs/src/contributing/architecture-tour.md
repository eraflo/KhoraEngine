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

---

Got the map? Now learn the rules every change must follow:
[Conventions](./conventions.md).
