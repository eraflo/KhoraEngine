# Concepts

<p class="kp-lede">The "why" behind Khora. A game has about a sixtieth of a second to make each picture, and many jobs to fit into it. These pages explain how Khora shares that time between the jobs, and how each job chooses what to do with its share.</p>

They explain the ideas and the architecture — they don't walk you through tasks
(that's [How-to guides](../how-to/index.md)) or list every type (that's the
[API reference](../reference/api.md)). Read them in order for the full picture,
or jump to the one you need.

## The core ideas

1. [The big idea (SAA)](./saa.md) — why an engine would negotiate with itself.
2. [Architecture (CLAD)](./clad.md) — the Control → Agent → Lane → Data descent.
3. [The frame](./the-frame.md) — the per-frame loop, its phases and waves, fixed timestep, and render interpolation.

## The adaptive machinery

4. [Data and the ECS (CRPECS)](./ecs.md) — how entities and components are stored.
   - [Knowing what changed](./change-detection.md) — how the data layer tells what was written.
5. [Agents and Lanes](./agents-and-lanes.md) — the strategist that chooses, the lanes that run as layers.
   - [Families and levels](./families-and-levels.md) — one job, several ways of doing it, each priced as a row.
6. [GORNA](./gorna.md) — how the families share the frame's time.
   - [The protocol](./gorna-protocol.md) — offers, envelopes lent at a frame start, reports, epochs.
   - [The arbiter](./arbiter.md) — how the DCC decides: the cost book, the proposers, the shield.
7. [AGDF](./agdf.md) — adapting data layout at runtime, without changing meaning.
8. [Scripting (Ergon)](./scripting/index.md) — a gameplay language that can be told to stop.

## The subsystems

Each is a trait in `khora-core` with a default backend in `khora-infra`, and a
family of lanes in `khora-lanes`:

- [Rendering](./rendering.md) · [Physics](./physics.md) · [Audio](./audio.md) · [UI](./ui.md)
- [Assets](./assets.md) · [Residency](./residency.md) — an asset's copy kept where a device can use it
- [Serialization](./serialization.md) · [Scenes and game saves](./saves.md) · [Prefabs](./prefabs.md) · [Telemetry](./telemetry.md)

And one that is not a subsystem but guards them all: [Trust](./trust.md) — what a project you did not make may do, and how its files are checked.

---

Unfamiliar with a term? The [Glossary](../reference/glossary.md) defines the
vocabulary (SAA, CLAD, GORNA, AGDF, CRPECS, family, row, envelope, lane, layer,
Flow, …). Ready to build? Head to the [tutorials](../tutorials/your-first-game.md).
