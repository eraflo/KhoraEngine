# Concepts

The "why" behind Khora. These pages explain the ideas and the architecture —
they don't walk you through tasks (that's [How-to guides](../how-to/index.md))
or list every type (that's the [API reference](../reference/api.md)). Read them
in order for the full picture, or jump to the one you need.

## The core ideas

1. [The big idea (SAA)](./saa.md) — why an engine would negotiate with itself.
2. [Architecture (CLAD)](./clad.md) — the Control → Agent → Lane → Data descent.
3. [The frame](./the-frame.md) — the per-frame loop, fixed timestep, and render interpolation.

## The adaptive machinery

4. [Data and the ECS (CRPECS)](./ecs.md) — how entities and components are stored.
5. [Agents and Lanes](./agents-and-lanes.md) — strategists that choose, executors that run.
6. [GORNA](./gorna.md) — how subsystems negotiate for the frame budget.
7. [AGDF](./agdf.md) — adapting data layout at runtime, without changing meaning.
8. [Scripting (Ergon)](./scripting.md) — a gameplay language that can be told to stop.

## The subsystems

Each is a trait in `khora-core` with a default backend in `khora-infra`:

- [Rendering](./rendering.md) · [Physics](./physics.md) · [Audio](./audio.md) · [UI](./ui.md)
- [Assets](./assets.md) · [Serialization](./serialization.md) · [Telemetry](./telemetry.md)

---

Unfamiliar with a term? The [Glossary](../reference/glossary.md) defines the
vocabulary (SAA, CLAD, GORNA, AGDF, CRPECS, Lane, Flow, …). Ready to build?
Head to the [tutorials](../tutorials/your-first-game.md).
