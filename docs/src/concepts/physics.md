# Physics

<p class="kp-lede">A ball thrown in a game should fall, bounce and roll the same way every time it is thrown the same way. Physics is the part of the engine that makes things fall, collide and rest. Khora hands that work to a physics library behind one door, steps it at a steady rhythm whatever the screen does, and keeps apart what a designer placed and what the simulation made of it.</p>

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

Physics is one slice of the per-frame descent (`Control → Agent → Lane → Data`),
in three parts. Before the agents run, the `physics_provider_sync` DataSystem
registers the world's bodies and colliders with the provider. The physics lane then
locks the provider, calls `step` and resolves the step's contacts to entities; it
reads no View and never the World, and reaches the provider only through its trait.
`PhysicsFlow` publishes the physics statistics. At the
end of the frame, the `physics_world_writeback` DataSystem reads the new poses back
into the World.

```
ECS (bodies, colliders, transforms)
  ↓ physics_provider_sync (PreExtract): bodies and colliders into the provider
physics lane
  ↓ PhysicsProvider::step(dt), contacts resolved to entities
  ↓ the deck: PhysicsStepResult, ContactBatch
  ↓ physics_world_writeback (Maintenance): poses back into the World
  ↓ collision_dispatch (Maintenance, after it): contacts into Channel<Collision>
```

A debug lane that visualizes collision shapes is opt-in and switched on from the
editor; the standard step lane is the only required one.

**Contacts are lane output.** A contact relates two entities, so it is no
component, and it is not a callback from inside the step either: the lane writes
the step's contacts, each naming two entities, into its deck slot `ContactBatch`.
At the end of the frame `collision_dispatch` moves them into the runtime's
`Channel<Collision>` — after the writeback, so a reader reacting to a contact sees
the poses the step ended at. Gameplay reads the channel under its own budget: the
scripts' `Touched` and `Separated` events are one reader (`collision_to_script`). A frame whose lane
did not run (paused, no provider) is not a frame with no contacts: its slot stays
empty and nothing is dispatched. The channel keeps the last 4096 contacts
(`COLLISION_BACKLOG`) for a reader that has not caught up, dropping the oldest.

**Every body needs a world pose.** The sync and the writeback skip an entity with
no `GlobalTransform`, and a scene holds none (it is `Derived`). So
`ensure_global_transform` (`PostSimulation`, first in the phase) gives one to every
entity with a `Transform`, seeded from its local pose: a body loaded from a scene is
simulated from its first frame.

**A scope does not reach physics.** A family switched on for one marker component
only (`Activation::scope`) runs on those entities alone; the physics provider steps
every body it was given, so a scoped physics family runs plainly on.

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

A body's velocity is given once. `RigidBody::initial_velocity` (and
`initial_angular_velocity`) is applied when the body is added to the provider; the
sync never writes it again, or it would undo every frame what gravity added.
Editing it on a body already simulated changes nothing, and the provider has no
call to set a velocity yet. The velocity the body *has* is read back into
`BodyMotion`.

A collider with `is_sensor` set is a **trigger**: a volume that notices, not
matter.

- It always reports what enters it and leaves it, whatever its events flag says.
- It has no mass: it never moves its body's centre of mass.
- It sees dynamic and kinematic bodies (a platform entering it), never fixed ones,
  so a trigger lying on the floor does not report the level at spawn.
- A character controller walks into it rather than stopping at its edge.

## Who owns a pose

One field cannot hold two facts — what the designer declared, and what gravity made
of it. A scene saved mid-play would record the simulated pose over the authored
one, the inspector would show a number nobody typed, and a gizmo drag would anchor
on wherever the body had fallen to.

`ComponentProvenance` says who may write a component, and the pose is split
along it:

| Component | Provenance | Holds | Serialized |
|---|---|---|---|
| `Transform` | `Authored` | what a human, a tool or game code declared | yes |
| `SimulatedTransform` | `Runtime` | the **world** pose the provider reports | no |
| `GlobalTransform` | `Derived` | the composed world pose the renderer reads | no |
| `BodyMotion` | `Runtime` | the velocity the body *has* | no |
| `RigidBody::initial_velocity` | `Authored` | the velocity it was *given*, applied once | yes |

`SimulatedTransform` is deliberately **not** composed with a parent: a body
parented to a moving platform is simulated in world space. Writing that world pose
into the *local* `Transform` and letting propagation multiply it by the parent
again would make a parented body drift by its parent's transform every frame.

It is `Runtime` rather than `Derived` because a pose after five seconds of
falling is not recomputable from authored data — it depends on the path taken,
on every contact along the way, on the integrator. That is what `Runtime` means,
and it is what keeps a simulated pose out of a scene file without a line of
filtering, because the serializer already honours the axis.

**When the simulation lets go.** An entity that loses its `RigidBody` (or a
character controller's collider) is handed back to its author where it is drawn:
`physics_release` (`PostSimulation`, after the transform propagation) writes into
its `Transform` the local pose that composes, under its parent as it is this
frame, to the pose it is drawn at, and removes its `SimulatedTransform` and
`BodyMotion`. It is the only write physics makes to an `Authored` component;
dropping the simulated pose instead would send the body back to where the level
placed it. A move declared the same frame (`Teleported`) wins, and the author's
`Transform` is kept. A mirrored entity keeps the signs of scale its author typed.

## Moving something is declared, not inferred

A teleport cannot be inferred by comparing the provider's pose with the entity's.
Integration drift, a parent-frame mismatch, a sub-step overshoot and a real
teleport all produce "the two values differ", and no threshold separates them: a
centimetre is small enough to fire on a fast body's own motion and large enough to
silently discard a 3 mm nudge.

Moving something is an **act**, and whoever performs it knows. The gizmo, a
script's `SetPosition`, a tool: each adds the `Teleported` marker, and the sync
consumes it and removes it in the same frame. A flag that outlived its frame
would teleport a body back to its authored pose forever.

`Teleported` is a component rather than a message on a `Channel`: the editor's
gizmo drag runs in `EngineApp::update`, which holds the `World` but no `Runtime`,
so the marker is what every mover can reach.

## Fixed timestep is the determinism guarantee

Physics steps at a **fixed** timestep, a whole number of times per frame, decoupled
from the variable render rate. The physics family does **not** own an accumulator:
its step lives at its place (`Place::fixed_step`), and the scheduler drives the
cadence. Each frame it hands the family exactly the simulated time the game's ticks
advanced, computes how many whole steps fit, and runs the family's installed layer
that many times (the remainder waits for the next frame). The accumulator arithmetic and the
spiral-of-death clamp are part of the engine's single frame loop, explained in
[The frame](./the-frame.md).

Determinism is the entire reason for this: the same total elapsed time produces the
same number of steps regardless of frame cadence, which is what makes replays,
multiplayer, and reproducible bug reports possible. Variable steps cause subtle
simulation drift across machines, so they were considered and rejected.

The step is a **game setting**, never a GORNA lever: the game's `physics.step`
(`PhysicsSettings`), written at the place once at boot. A coarser step would change
what the simulation does — tunnelling, stacking, the feel of a jump — and adaptation
changes how the engine works, never what the game does. The physics family offers
one level; its agent stays a pure strategist that answers GORNA and installs its
layer, nothing more.

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

## Under the hood

What the Rapier backend guarantees each frame, in `khora-infra/src/physics/rapier/world.rs`:

- **A collider is written only when something changed.** The sync describes every
  collider every frame; the backend compares first (pose, shape, sensor, collision
  types, events, friction, restitution) and touches the collider only on a
  difference. In Rapier, taking a collider mutably marks it modified and keeps its
  body awake, so a collider written every frame would stop every body in the scene
  from ever sleeping. Shapes and poses compare within what a frame's sync can
  produce from unchanged authored data: a relative 10⁻⁵ on sizes, so a large shape
  turning under a world matrix is not rebuilt each frame.
- **An attached collider is written its offset from its body only.** Setting its
  world pose would overwrite the one the solver derives from the body, and a step's
  contacts would be solved at another pose than the one they were found at.
- **A new surface wakes what rests on it.** Rapier flags no change for a new
  friction or restitution, so the backend wakes the collider's body and every body
  touching it: a crate asleep on a slope made frictionless slides.
- **A collider whose shape changes while it touches something is replaced.** Rapier
  keeps a contact pair's old points across a shape change, so a body grown in place
  would stay sunk in the ground. The backend builds a new collider; the engine's
  handle names the new one through an alias table, and the `Stopped`/`Started` pair
  the swap raises with what it still touches is dropped, so no script hears a false
  separation.
- **A collider carries its owner.** The entity that made it is packed into Rapier's
  `user_data` when the collider is created, and `PhysicsProvider::entity_of` reads
  it back from the collider itself. No index has to be kept in step with the world:
  a handle whose slot was recycled does not resolve, and a dead entity's collider
  is gone with it.

## Next steps

- [The frame](./the-frame.md) — the fixed-timestep sub-loop physics is stepped by.
- [Data and the ECS](./ecs.md) — how physics components are stored and queried.
- [SAA](./saa.md) — what GORNA adapts, and why the physics step is not part of it.
- [Glossary](../reference/glossary.md) — PhysicsProvider, fixed timestep, agent, lane.
