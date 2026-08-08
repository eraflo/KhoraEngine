# Physics

Rigid-body simulation behind a single trait, stepped on a deterministic fixed
clock. This page explains *why* physics is a swappable provider and how it plugs
into the CLAD descent and GORNA budgeting. It is an explanation, not a recipe —
for the steps to add a body and a collider, see the rustdoc on the physics
components in `khora_data::ecs`; the model is described below.

---

## The contract is one trait

The entire physics surface is the **`PhysicsProvider`** trait in `khora-core`
(`physics/mod.rs`). It is a small, explicit contract: step the simulation, add and
remove bodies and colliders, read and set body transforms, cast rays, drain
collision events, resolve character movement. Nothing in the engine above it knows
about a concrete solver.

That matters because of the boundary rule it enforces: the physics
**[agent](../reference/glossary.md)** and **[lane](../reference/glossary.md)** never
call the backend directly — they call `PhysicsProvider`. The default implementation
is the Rapier3D backend in `khora-infra`; a future native solver drops in as a new
implementation of the same trait without touching agent or lane code. The trait
*is* the seam, and respecting it is what keeps the engine from coupling to one
physics library.

## How it plugs into a frame

Physics is one slice of the per-frame descent (`Control → Agent → Lane → Data`).
The data layer projects a physics View from the ECS into the LaneBus; the physics
lane consumes that View, syncs the relevant bodies and colliders into the provider,
calls `step`, and reads the updated transforms back. As with every domain, the lane
reads a projected View — never the World directly — and the provider is reached only
through its trait.

```
ECS (bodies, colliders, transforms)
  ↓ physics Flow projects a View into the LaneBus
physics lane
  ↓ sync into the provider, then PhysicsProvider::step(dt)
  ↓ read updated poses back out
```

A debug lane that visualizes collision shapes is opt-in and switched on from the
editor; the standard step lane is the only required one.

## Components describe, the provider simulates

Physics entities carry two domain components — a **rigid body** (its type:
dynamic, static, or kinematic; its mass, velocity, and a continuous-collision flag)
and a **collider** (a shape — box, sphere, or capsule — plus friction and
restitution).

The split between *dynamic*, *static*, and *kinematic* is the load-bearing
distinction: dynamic bodies respond to forces and collisions, static bodies are
immovable terrain, and kinematic bodies are moved by code yet still push dynamic
ones. Continuous collision detection is opt-in per body — it catches tunneling for
fast, small objects at the cost of step time, so it is off by default and enabled
only where it earns its keep.

## Who owns a pose

The writeback used to overwrite `Transform` every frame, so one field held two
different facts — what the designer declared, and what gravity made of it — and
nothing could tell them apart. Saving a scene mid-play recorded the simulated
pose over the authored one; the inspector showed a number nobody typed; a gizmo
drag anchored on wherever the body had fallen to.

`ComponentProvenance` already said who may write a component. Applying it splits
what one field held:

| Component | Provenance | Holds | Serialized |
|---|---|---|---|
| `Transform` | `Authored` | what a human, a tool or game code declared | yes |
| `SimulatedTransform` | `Runtime` | the **world** pose the provider reports | no |
| `GlobalTransform` | `Derived` | the composed world pose the renderer reads | no |
| `BodyMotion` | `Runtime` | the velocity the body *has* | no |
| `RigidBody::initial_velocity` | `Authored` | the velocity it was *given*, applied once | yes |

`SimulatedTransform` is deliberately **not** composed with a parent: a body
parented to a moving platform is simulated in world space, and writing that world
pose into the *local* `Transform` and letting propagation multiply it by the
parent again is what made a parented body drift by its parent's transform every
frame.

It is `Runtime` rather than `Derived` because a pose after five seconds of
falling is not recomputable from authored data — it depends on the path taken,
on every contact along the way, on the integrator. That is what `Runtime` means,
and it is what keeps a simulated pose out of a scene file without a line of
filtering, because the serializer already honours the axis.

## Moving something is declared, not inferred

The sync used to *guess* a teleport: it compared the provider's pose to the
entity's and, past a centimetre, assumed somebody had moved it. That cannot
work. Integration drift, a parent-frame mismatch, a sub-step overshoot and a
real teleport all produce "the two values differ", and no threshold separates
them — one centimetre was small enough to fire on a fast body's own motion and
large enough to silently discard a 3 mm nudge.

Moving something is an **act**, and whoever performs it knows. The gizmo, a
script's `SetPosition`, a tool: each adds the `Teleported` marker, and the sync
consumes it and removes it in the same frame. A flag that outlived its frame
would teleport a body back to its authored pose forever.

## Fixed timestep is the determinism guarantee

Physics steps at a **fixed** timestep, a whole number of times per frame, decoupled
from the variable render rate. The physics agent does **not** own an accumulator —
it advertises its step duration and the scheduler drives the cadence: each frame it
accumulates the clamped real delta, computes how many whole fixed steps fit, and
invokes the agent that many times. The accumulator arithmetic and the
spiral-of-death clamp are part of the engine's single frame loop, explained in
[The frame](./the-frame.md).

Determinism is the entire reason for this: the same total elapsed time produces the
same number of steps regardless of frame cadence, which is what makes replays,
multiplayer, and reproducible bug reports possible. Variable steps cause subtle
simulation drift across machines, so they were considered and rejected.

The step rate is itself a **GORNA** negotiation surface. Under a healthy budget the
agent advertises a finer step; under pressure it advertises a coarser one, so the
scheduler runs fewer sub-steps per frame rather than dropping the frame entirely.
The transition is graceful — bodies keep their state, only the cadence changes. The
agent stays a pure strategist: it chooses a step rate given a budget and dispatches
its lane, nothing more.

## The backends

`khora-infra/src/physics/` holds one subfolder per backend.

### The default — Rapier3D

The default `PhysicsProvider` is a Rapier3D wrapper in
`khora-infra/src/physics/rapier/`. It translates Khora's body, collider, and math
types into Rapier's and back, and routes raycasts through Rapier's query pipeline.
It is genuinely swappable: another solver implements the same trait and replaces
it without the agent or lane noticing. That is the payoff of putting a trait at
the boundary instead of calling the library directly.

### The in-house one — started, not finished

`khora-infra/src/physics/khora/` holds three stages of a simulation step: an
incremental AABB BVH for the broad phase, a narrow phase, and an impulse solver.
It **does not implement `PhysicsProvider`** — nothing stitches them into a world
the engine can step, so Rapier is what runs.

It exists because Rapier is a general-purpose engine and the goal is eventually
to do things it does not: the roadmap's [Phase 6](../roadmap.md) targets a
unified MLS-MPM / IPC / XPBD approach. Saying so here matters, because code with
no callers reads as abandoned unless something says otherwise. It is also almost
entirely untested, which is the honest near-term risk on it.

## Next steps

- [The frame](./the-frame.md) — the fixed-timestep sub-loop physics is stepped by.
- [Data and the ECS](./ecs.md) — how physics components are stored and queried.
- [SAA](./saa.md) — why the step rate is a budget GORNA negotiates.
- [Glossary](../reference/glossary.md) — PhysicsProvider, fixed timestep, agent, lane.
