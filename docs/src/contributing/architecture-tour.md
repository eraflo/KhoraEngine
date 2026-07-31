# Architecture tour

The [Concepts](../concepts/index.md) section explains every idea in Khora, but
when you're contributing you want them in a specific order — the one that builds
the mental model fastest, from the philosophy down to the subsystem you're about
to touch. This page is that thread. It adds no new architecture content; it
points you into the existing concept pages in the order that pays off.

Read these in sequence. Each line tells you **what you'll get** from the page.

## The core mental model

1. **[The big idea — SAA](../concepts/saa.md)**
   *What you'll get:* why an engine would negotiate with itself — the Symbiotic
   Adaptive Architecture, where subsystems are intelligent agents that bargain for
   resources rather than running fixed code paths.

2. **[Architecture — CLAD](../concepts/clad.md)**
   *What you'll get:* the `Control → Agent → Lane → Data` descent that names the
   command path of a frame, and the strict downward dependency direction every
   crate obeys.

3. **[The frame](../concepts/the-frame.md)**
   *What you'll get:* the per-frame loop end to end — fixed timestep, render
   interpolation, and where the CLAD descent actually happens each tick. This is
   the spine everything else hangs off.

## The adaptive machinery

4. **[Data and the ECS — CRPECS](../concepts/ecs.md)**
   *What you'll get:* how entities and components are stored, queried, and laid out
   — the data layer at the bottom of the descent.

5. **[Agents and Lanes](../concepts/agents-and-lanes.md)**
   *What you'll get:* the split between strategists that *choose* (agents) and
   executors that *run* (lanes), and why agents own no per-frame state.

6. **[GORNA](../concepts/gorna.md)**
   *What you'll get:* the negotiation protocol — how agents bid for the frame
   budget and how the controller arbitrates. This is the heart of SAA in practice.

## Then: the subsystem you care about

With the spine in place, dive into the one (or two) subsystems your change
touches. Each is a `khora-core` trait with a default backend in `khora-infra`:

- [Rendering](../concepts/rendering.md) · [Physics](../concepts/physics.md) ·
  [Audio](../concepts/audio.md) · [UI](../concepts/ui.md)
- [Assets](../concepts/assets.md) · [Serialization](../concepts/serialization.md) ·
  [Telemetry](../concepts/telemetry.md)

If your work is about data layout adaptation, also read
[AGDF](../concepts/agdf.md) — how the data layer changes its representation at
runtime without ever changing meaning ("adapt the HOW, never the WHAT").

## Navigating the code itself

Two facts make the codebase legible once you start reading it:

- **Dependencies flow strictly downward**: `khora-core` → `khora-data` /
  `khora-control` → `khora-lanes` → `khora-agents` → `khora-infra` →
  `khora-sdk`. There are no cycles. Abstract traits live in `khora-core`;
  concrete backends live under `khora-infra`. Knowing this tells you, for any
  symbol, roughly *which crate* it must be in.
- **Query the codegraph before grepping.** The repo ships a codegraph index of
  every symbol and edge. Use it to jump straight to a definition, find callers, or
  see what a change would impact, instead of text-searching the tree.

---

Got the map? Now learn the rules every change must follow:
[Conventions](./conventions.md).
